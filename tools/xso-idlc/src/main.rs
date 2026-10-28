pub mod ast;
pub mod write;
pub mod bridge_ir;
pub mod codegen;
pub mod diag;
pub mod lexer;
pub mod parser;
pub mod semantic;
pub mod deps;
pub mod cli;
pub mod interactive;
pub mod migrate;
pub mod sbom;
mod tests;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::panic;

/// 生成器版本（增量/缓存失效）
pub const GEN_VERSION: &str = "3";
/// 源字节上限（资源限制，防 DoS）
const MAX_SRC_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Out {
    Anchor,
    Cpp,
    Rust,
    Doc,
    Test,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            crate::error!("{}", e);
            ExitCode::FAILURE
        }
    }
}

/// 旧式纯 flag 调用入口：xso-idlc --input a.xso [options]
/// 子命令（gen/check/...）经 cli::dispatch 解析后也会转译为此 argv 复用本逻辑。
pub fn run(args: &[String]) -> diag::Result<()> {
    if args.len() <= 1 { return interactive::run_interactive(); }

    // 子命令形态 `xso-idlc <cmd> [options]`（含 `help <cmd>`）：交给 cli 统一解析，
    // 由 cli 转译为旧式 argv 后回到本函数；以 `-` 开头的才是本函数的纯 flag 形态。
    if !args[1].starts_with('-') { return cli::dispatch(&args); }

    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut anchor_out = None;
    let mut cpp_out = None;
    let mut rust_out = None;
    let mut doc_out = None;
    let mut test_out = None;
    let mut emit_ir: Option<PathBuf> = None;
    let mut sbom_out: Option<PathBuf> = None;
    let mut cache_dir = PathBuf::from(".xso-cache");
    // 多文件依赖的模块搜索目录（--include-dir，可重复）
    let mut include_dirs: Vec<PathBuf> = Vec::new();
    let mut check_only = false;
    let mut migrate_path: Option<PathBuf> = None;
    let mut interactive_flag = false;
    let mut jobs: usize = std::thread::available_parallelism()
        .map(|n| n.get()).unwrap_or(4);
    let mut no_cache = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--input"    => { inputs.push(PathBuf::from(&args[i + 1])); i += 2; }
            "--anchor"   => { anchor_out = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--cpp-out"  => { cpp_out    = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--rust-out" => { rust_out   = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--doc-out"  => { doc_out    = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--test-out" => { test_out   = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--emit-ir"  => { emit_ir    = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--sbom"     => { sbom_out   = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--cache-dir"=> { cache_dir  = PathBuf::from(&args[i + 1]); i += 2; }
            "--include-dir" => { include_dirs.push(PathBuf::from(&args[i + 1])); i += 2; }
            "--jobs"     => {
                if let Ok(n) = args[i + 1].parse::<usize>() { jobs = n.max(1); }
                i += 2;
            }
            "--no-cache" => { no_cache   = true; i += 1; }
            "--check"    => { check_only = true; i += 1; }
            "--migrate"  => { migrate_path = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--interactive" | "-i" => { interactive_flag = true; i += 1; }
            "--help" | "-h" => { cli::print_general_help(); return Ok(()); }
            "--version" | "-V" => { crate::out!("xso-idlc {}", GEN_VERSION); return Ok(()); }
            other => return Err(diag::CompileError::new(format!("unknown argument: {}", other))),
        }
    }

    if interactive_flag { return interactive::run_interactive(); }
    if inputs.is_empty() {
        return Err(diag::CompileError::new("missing --input <file.xso>"));
    }
    if let Some(p) = &migrate_path {
        return run_migrate(p);
    }

    // ===== 多文件依赖校验（工程级门禁）=====
    // 多个输入构成一个工程：先解析并校验跨文件依赖（模块解析 / 符号存在 /
    // 自我导入 / 重复导入 / 依赖环 / 重复模块名），任一错误即中止构建；
    // 通过后得到每个输入需要内联的外部定义与依赖签名。
    let (linked, signatures) = if inputs.len() > 1 {
        let report = deps::analyze(&inputs, &deps::Options {
            strict: false,
            include_dirs: include_dirs.clone(),
        });
        for dg in &report.diagnostics {
            match dg.severity {
                deps::Severity::Error => crate::error!("{}", dg.render()),
                deps::Severity::Warning => crate::warn!("{}", dg.render()),
            }
        }
        if report.has_errors() {
            return Err(diag::CompileError::new(format!(
                "multi-file dependency check failed: {} error(s) (see above)",
                report.errors())));
        }
        crate::progress!("dependency check OK: {} module(s), build order: {}",
            report.modules.len(),
            if report.order.is_empty() { "(none)".to_string() } else { report.order.join(" -> ") });
        (report.linked, report.signatures)
    } else {
        (std::collections::BTreeMap::new(), std::collections::BTreeMap::new())
    };

    let multi = inputs.len() > 1;
    let errors: Mutex<Vec<diag::CompileError>> = Mutex::new(Vec::new());
    let lnk = &linked;
    let sig = &signatures;

    // 跨输入并行构建：按 --jobs 上限分桶
    std::thread::scope(|s| {
        for chunk in inputs.chunks(jobs.max(1)) {
            let errors = &errors;
            let a = &anchor_out;
            let c = &cpp_out;
            let r = &rust_out;
            let d = &doc_out;
            let t = &test_out;
            let e = &emit_ir;
            let s2 = &sbom_out;
            let cd = &cache_dir;
            s.spawn(move || {
                for input in chunk {
                    process_input(input, multi, a, c, r, d, t, e, s2, cd,
                                  check_only, no_cache, lnk, sig, errors);
                }
            });
        }
    });

    let errs = errors.into_inner().unwrap();
    if !errs.is_empty() {
        crate::progress!("{} error(s):", errs.len());
        for e in &errs { crate::error!("  {}", e); }
        return Err(diag::CompileError::new("generation failed; see errors above"));
    }
    Ok(())
}

/// 单个输入的处理：解析 -> 增量缓存判定 -> 并行生成各后端 -> 事务写出 -> SBOM/IR/缓存。
fn process_input(
    input: &PathBuf,
    multi: bool,
    anchor_out: &Option<PathBuf>,
    cpp_out: &Option<PathBuf>,
    rust_out: &Option<PathBuf>,
    doc_out: &Option<PathBuf>,
    test_out: &Option<PathBuf>,
    emit_ir: &Option<PathBuf>,
    sbom_out: &Option<PathBuf>,
    cache_dir: &PathBuf,
    check_only: bool,
    no_cache: bool,
    // 多文件链接结果：每个输入需要从被导入模块内联进来的定义
    linked: &std::collections::BTreeMap<PathBuf, Vec<crate::ast::Item>>,
    // 多文件依赖签名：依赖内容变化要让增量缓存失效
    signatures: &std::collections::BTreeMap<PathBuf, u64>,
    errors: &Mutex<Vec<diag::CompileError>>,
) {
    let deps_hash = signatures.get(input).copied().unwrap_or(0);
    let (ir_opt, perrs) = parse_and_check(input, linked);
    if !perrs.is_empty() { errors.lock().unwrap().extend(perrs); }
    let ir = match ir_opt {
        Some(ir) => Arc::new(ir),
        None => return,
    };
    crate::progress!("parsed '{}' v{} ({} items), fingerprint=0x{:08x}",
              ir.module.name, ir.module.version, ir.module.items.len(), ir.fingerprint());
    crate::progress!("semantic OK; bridge IR v{}: {} layouts, {} opaque",
              ir.ir_version, ir.type_layouts.len(), ir.opaque_types.len());

    let jobs = collect_jobs(input, multi, anchor_out, cpp_out, rust_out, doc_out, test_out);

    // 增量缓存：输入哈希 + 依赖签名 + 版本一致且产物齐全 -> 整轮跳过
    // （check 模式与 --no-cache 不查缓存）
    if !check_only && !no_cache && cache_hit(input, &ir, &jobs, cache_dir, deps_hash) {
        crate::progress!("cache hit, skipping generation for {}", input.display());
        return;
    }

    // 多线程生成（后端相互独立）；panic 隔离：单后端 panic 转为错误而非崩进程
    let produced: Mutex<Vec<(PathBuf, Option<String>)>> = Mutex::new(Vec::new());
    {
        let produced = &produced;
        std::thread::scope(|s| {
            for (kind, path) in &jobs {
                let ir = Arc::clone(&ir);
                s.spawn(move || {
                    let code = panic::catch_unwind(panic::AssertUnwindSafe(|| generate(*kind, &ir)));
                    match code {
                        Ok(c) => produced.lock().unwrap().push((path.clone(), Some(c))),
                        Err(_) => {
                            produced.lock().unwrap().push((path.clone(), None));
                            errors.lock().unwrap().push(diag::CompileError::new(format!(
                                "internal panic while generating {}", path.display())));
                        }
                    }
                });
            }
        });
    }

    let mut ordered = produced.into_inner().unwrap();
    ordered.sort_by(|a, b| a.0.cmp(&b.0));

    // 校验 / 写出（事务原子写 + 失败回滚）
    let mut sbom_files: Vec<(String, String)> = Vec::new();
    let mut tx = WriteTx::new();
    let mut tx_failed = false;
    for (path, code_opt) in &ordered {
        let code = match code_opt {
            Some(c) => c,
            None => { tx_failed = true; continue; }
        };
        if check_only {
            let on_disk = fs::read_to_string(path).unwrap_or_default();
            if on_disk != *code {
                errors.lock().unwrap().push(diag::CompileError::new(format!(
                    "{} is out of date; re-run xso-idlc without --check", path.display())));
            }
        } else if let Err(e) = tx.stage(path, code) {
            errors.lock().unwrap().push(e);
            tx_failed = true;
        }
        if sbom_out.is_some() {
            sbom_files.push((path.display().to_string(), code.clone()));
        }
    }
    if tx_failed {
        tx.rollback();
    }

    // SBOM：产物清单 + SHA-256（供应链审计 / 完整性追踪）
    if let Some(p) = sbom_out {
        let json = sbom::generate(GEN_VERSION, &ir, &sbom_files);
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() { let _ = fs::create_dir_all(parent); }
        }
        match fs::write(p, json) {
            Ok(()) => crate::progress!("wrote SBOM {}", p.display()),
            Err(e) => errors.lock().unwrap().push(diag::CompileError::new(
                format!("cannot write {}: {}", p.display(), e))),
        }
    }

    if let Some(p) = emit_ir {
        if let Err(e) = emit_ir_file(p, &ir) { errors.lock().unwrap().push(e); }
    }

    if !check_only && !no_cache { write_cache(input, &ir, &jobs, cache_dir, deps_hash); }
}

/// 解析 + 语义检查 + 构建 Bridge IR。
/// 带回退恢复：一次收集全部语法/语义错误，每条都带文件、行列与源码片段。
fn parse_and_check(
    input: &PathBuf,
    linked: &std::collections::BTreeMap<PathBuf, Vec<crate::ast::Item>>,
) -> (Option<bridge_ir::BridgeIr>, Vec<diag::CompileError>) {
    let mut errs: Vec<diag::CompileError> = Vec::new();
    let src = match fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => return (None, vec![diag::CompileError::new(
            format!("cannot read {}: {}", input.display(), e))]),
    };
    if src.len() > MAX_SRC_BYTES {
        return (None, vec![diag::CompileError::new(format!(
            "input too large: {} bytes (limit {})", src.len(), MAX_SRC_BYTES))]);
    }

    let (module_opt, mut perrs) = parser::parse_with_recovery(&src);
    for e in perrs.drain(..) {
        errs.push(e.in_file(input.display().to_string()));
    }

    let mut module = match module_opt { Some(m) => m, None => return (None, errs) };
    // 多文件链接：把 `use` 导入的符号（含其传递依赖）内联进本模块，
    // 使每个模块的产物自包含（布局算法一致 ⇒ 两端 ABI 仍一致）。
    if let Some(extra) = linked.get(input) {
        if !extra.is_empty() { module.items.extend(extra.iter().cloned()); }
    }
    match semantic::analyze(&mut module) {
        Ok(()) => {}
        Err(e) => { errs.push(e.in_file(input.display().to_string())); return (None, errs); }
    }
    (Some(bridge_ir::BridgeIr::build(&module)), errs)
}

/// 单个后端生成（返回代码字符串；codegen 内部不会失败，IO 在写阶段处理）
fn generate(kind: Out, ir: &bridge_ir::BridgeIr) -> String {
    match kind {
        Out::Anchor => codegen::anchor::generate(ir),
        Out::Cpp => codegen::cpp::generate(ir),
        Out::Rust => codegen::rust::generate(ir),
        Out::Doc => codegen::doc::generate(ir),
        Out::Test => codegen::test::generate(ir),
    }
}

/// 收集某输入需要生成的（后端, 输出路径）
fn collect_jobs(
    input: &PathBuf,
    multi: bool,
    anchor_out: &Option<PathBuf>,
    cpp_out: &Option<PathBuf>,
    rust_out: &Option<PathBuf>,
    doc_out: &Option<PathBuf>,
    test_out: &Option<PathBuf>,
) -> Vec<(Out, PathBuf)> {
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "out".into());
    let dir = input.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| std::path::Path::new("."));

    let pick = |kind: Out, explicit: &Option<PathBuf>, suffix: &str| -> Option<(Out, PathBuf)> {
        if !multi {
            explicit.clone().map(|p| (kind, p))
        } else {
            Some((kind, dir.join(format!("{}{}", stem, suffix))))
        }
    };
    let mut jobs = Vec::new();
    if let Some(j) = pick(Out::Anchor, anchor_out, ".anchor.h") { jobs.push(j); }
    if let Some(j) = pick(Out::Cpp, cpp_out, ".h") { jobs.push(j); }
    if let Some(j) = pick(Out::Rust, rust_out, ".rs") { jobs.push(j); }
    if let Some(j) = pick(Out::Doc, doc_out, ".md") { jobs.push(j); }
    if let Some(j) = pick(Out::Test, test_out, ".test.rs") { jobs.push(j); }
    jobs
}

/// 写入事务：原子写（临时文件 + rename），并记录旧内容以便失败回滚
struct WriteTx {
    /// (路径, 原内容；None 表示原先不存在)
    entries: Vec<(PathBuf, Option<String>)>,
}

impl WriteTx {
    fn new() -> Self { Self { entries: Vec::new() } }

    /// 原子写入单个产物；内容未变则跳过（增量）
    fn stage(&mut self, path: &Path, code: &str) -> diag::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() { let _ = fs::create_dir_all(parent); }
        }
        let prev = fs::read_to_string(path).ok();
        self.entries.push((path.to_path_buf(), prev.clone()));
        if prev.as_deref() == Some(code) {
            crate::progress!("up-to-date {}", path.display());
            return Ok(());
        }
        let tmp = path.with_extension(format!("xso-tmp-{}", std::process::id()));
        fs::write(&tmp, code).map_err(|e| diag::CompileError::new(
            format!("cannot write {}: {}", tmp.display(), e)))?;
        fs::rename(&tmp, path).map_err(|e| diag::CompileError::new(
            format!("cannot rename {} -> {}: {}", tmp.display(), path.display(), e)))?;
        crate::progress!("wrote {}", path.display());
        Ok(())
    }

    /// 回滚：把所有已 stage 的文件恢复到写入前状态（原先不存在则删除）
    fn rollback(&self) {
        for (path, prev) in &self.entries {
            match prev {
                Some(c) => { let _ = fs::write(path, c); }
                None => { let _ = fs::remove_file(path); }
            }
        }
    }
}

/// 导出版本化 IR（JSON）；若已存在则比对指纹给出 diff 结论
fn emit_ir_file(path: &PathBuf, ir: &bridge_ir::BridgeIr) -> diag::Result<()> {
    let json = ir.to_json();
    if let Ok(old) = fs::read_to_string(path) {
        let old_fp = extract_fingerprint(&old);
        let new_fp = Some(ir.fingerprint());
        if old_fp != new_fp {
            crate::progress!("ABI CHANGE: fingerprint {:?} -> {:#010x}", old_fp, ir.fingerprint());
        } else {
            crate::progress!("no ABI change (fingerprint {:#010x})", ir.fingerprint());
        }
        if old != json {
            let added = json.lines().filter(|l| !old.lines().any(|o| o == *l)).count();
            let removed = old.lines().filter(|l| !json.lines().any(|n| n == *l)).count();
            crate::progress!("IR diff: +{} -{} lines", added, removed);
        }
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    fs::write(path, json)
        .map_err(|e| diag::CompileError::new(format!("cannot write {}: {}", path.display(), e)))?;
    crate::progress!("wrote IR {}", path.display());
    Ok(())
}

pub fn extract_fingerprint(json: &str) -> Option<u32> {
    for line in json.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("\"fingerprint\":") {
            let v = rest.trim().trim_end_matches(',').trim();
            return v.parse::<u32>().ok();
        }
    }
    None
}

/// 迁移模式：把旧 IDL 语法改写为当前语法（规则幂等）
pub fn run_migrate(path: &PathBuf) -> diag::Result<()> {
    let src = fs::read_to_string(path)
        .map_err(|e| diag::CompileError::new(format!("cannot read {}: {}", path.display(), e)))?;
    let m = migrate::migrate(&src);
    if !m.changed {
        crate::progress!("no migration needed: {}", path.display());
        return Ok(());
    }
    for r in &m.applied { crate::progress!("applied: {}", r); }
    fs::write(path, &m.content)
        .map_err(|e| diag::CompileError::new(format!("cannot write {}: {}", path.display(), e)))?;
    crate::progress!("migrated {}", path.display());
    Ok(())
}

// ===== 缓存（增量编译）=====

fn cache_path(dir: &Path, module: &str) -> PathBuf {
    dir.join(format!("{}.cache", module))
}

fn cache_hit(
    input: &Path,
    ir: &bridge_ir::BridgeIr,
    jobs: &[(Out, PathBuf)],
    dir: &Path,
    // 依赖签名：依赖文件内容变化时必须失效（跨文件依赖）
    deps_hash: u64,
) -> bool {
    let p = cache_path(dir, &ir.module.name);
    let Ok(text) = fs::read_to_string(&p) else { return false; };
    let mut in_hash = None;
    let mut ir_v = None;
    let mut gen_v = None;
    let mut dep_h: Option<String> = None;
    let mut outs: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("input_hash=") { in_hash = Some(v.to_string()); }
        else if let Some(v) = line.strip_prefix("ir_version=") { ir_v = Some(v.to_string()); }
        else if let Some(v) = line.strip_prefix("gen_version=") { gen_v = Some(v.to_string()); }
        else if let Some(v) = line.strip_prefix("deps_hash=") { dep_h = Some(v.to_string()); }
        else if let Some(v) = line.strip_prefix("out=") { outs.push(v.to_string()); }
    }
    let Ok(src) = fs::read_to_string(input) else { return false; };
    let want = format!("{:016x}", fnv1a(&src));
    if in_hash.as_deref() != Some(want.as_str()) { return false; }
    if dep_h.as_deref() != Some(format!("{:016x}", deps_hash).as_str()) { return false; }
    if ir_v.as_deref() != Some(format!("{}", ir.ir_version).as_str()) { return false; }
    if gen_v.as_deref() != Some(GEN_VERSION) { return false; }
    let cur: Vec<String> = jobs.iter().map(|(_, p)| p.display().to_string()).collect();
    if cur != outs { return false; }
    outs.iter().all(|o| Path::new(o).exists())
}

fn write_cache(
    input: &Path,
    ir: &bridge_ir::BridgeIr,
    jobs: &[(Out, PathBuf)],
    dir: &Path,
    deps_hash: u64,
) {
    let Ok(src) = fs::read_to_string(input) else { return; };
    let _ = fs::create_dir_all(dir);
    let mut s = String::new();
    s.push_str(&format!("input_hash={:016x}\n", fnv1a(&src)));
    s.push_str(&format!("deps_hash={:016x}\n", deps_hash));
    s.push_str(&format!("ir_version={}\n", ir.ir_version));
    s.push_str(&format!("gen_version={}\n", GEN_VERSION));
    for (_, p) in jobs { s.push_str(&format!("out={}\n", p.display())); }
    let _ = fs::write(cache_path(dir, &ir.module.name), s);
}

pub fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
