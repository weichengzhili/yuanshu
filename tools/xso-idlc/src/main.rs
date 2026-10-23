mod ast;
mod bridge_ir;
mod codegen;
mod diag;
mod lexer;
mod parser;
mod semantic;
mod sbom;
mod migrate;
mod interactive;
mod cli;
#[cfg(test)]
mod tests;

use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

/// 生成器版本（增量/缓存失效）
pub(crate) const GEN_VERSION: &str = "3";
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
    match cli::dispatch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => { eprintln!("{}", e); ExitCode::FAILURE }
    }
}

/// 一次调用的全局选项
struct RunCtx {
    multi: bool,
    anchor_out: Option<PathBuf>,
    cpp_out: Option<PathBuf>,
    rust_out: Option<PathBuf>,
    doc_out: Option<PathBuf>,
    test_out: Option<PathBuf>,
    emit_ir: Option<PathBuf>,
    sbom_out: Option<PathBuf>,
    cache_dir: PathBuf,
    check_only: bool,
    no_cache: bool,
}

pub(crate) fn run(args: &[String]) -> diag::Result<()> {
    if args.len() <= 1 { return interactive::run_interactive(); }

    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut migrate_path: Option<PathBuf> = None;
    let mut interactive_flag = false;
    let mut anchor_out = None; let mut cpp_out = None; let mut rust_out = None;
    let mut doc_out = None; let mut test_out = None;
    let mut emit_ir: Option<PathBuf> = None;
    let mut sbom_out: Option<PathBuf> = None;
    let mut cache_dir = PathBuf::from(".xso-cache");
    let mut check_only = false;
    let mut no_cache = false;
    let mut jobs: Option<usize> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--input" => { inputs.push(PathBuf::from(&args[i + 1])); i += 2; }
            "--anchor" => { anchor_out = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--cpp-out" => { cpp_out = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--rust-out" => { rust_out = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--doc-out" => { doc_out = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--test-out" => { test_out = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--emit-ir" => { emit_ir = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--sbom" => { sbom_out = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--migrate" => { migrate_path = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--interactive" | "-i" => { interactive_flag = true; i += 1; }
            "--cache-dir" => { cache_dir = PathBuf::from(&args[i + 1]); i += 2; }
            "--check" => { check_only = true; i += 1; }
            "--no-cache" => { no_cache = true; i += 1; }
            "--jobs" => { jobs = args[i + 1].parse::<usize>().ok(); i += 2; }
            "--help" | "-h" => { cli::print_general_help(); return Ok(()); }
            "--version" | "-V" => { println!("xso-idlc {} (ir {})", GEN_VERSION, bridge_ir::IR_VERSION); return Ok(()); }
            other => return Err(diag::CompileError::new(format!("unknown argument: {}", other))),
        }
    }

    if interactive_flag { return interactive::run_interactive(); }
    if let Some(p) = &migrate_path { return run_migrate(p); }

    if inputs.is_empty() {
        return Err(diag::CompileError::new(
            "missing --input <file.xso> (or run with no args for interactive mode)"));
    }

    let ctx = RunCtx {
        multi: inputs.len() > 1,
        anchor_out, cpp_out, rust_out, doc_out, test_out,
        emit_ir, sbom_out, cache_dir, check_only, no_cache,
    };

    // ===== 并行构建：跨输入并行（受 --jobs 上限约束）=====
    let thread_n = jobs
        .filter(|n| *n > 0)
        .unwrap_or_else(|| std::thread::available_parallelism()
            .map(|n| n.get()).unwrap_or(4))
        .min(inputs.len().max(1));
    let per = (inputs.len() + thread_n - 1) / thread_n.max(1); // 每线程处理的输入数

    let errors: Mutex<Vec<diag::CompileError>> = Mutex::new(Vec::new());
    let errors_ref = &errors;
    let ctx = &ctx;
    std::thread::scope(|s| {
        for chunk in inputs.chunks(per) {
            s.spawn(move || {
                for input in chunk { process_input(input, ctx, errors_ref); }
            });
        }
    });

    let errs = errors.into_inner().unwrap();
    if !errs.is_empty() {
        eprintln!("[idlc] {} error(s):", errs.len());
        for e in &errs { eprintln!("  {}", e); }
        return Err(diag::CompileError::new("generation failed; see errors above"));
    }
    Ok(())
}

/// 处理单个输入：查增量缓存 -> (命中则跳过) -> 解析 -> 并行生成各后端 -> 事务化写入 -> 写缓存
fn process_input(input: &PathBuf, ctx: &RunCtx, errors: &Mutex<Vec<diag::CompileError>>) {
    let src = match fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => { errors.lock().unwrap().push(diag::CompileError::new(
            format!("cannot read {}: {}", input.display(), e))); return; }
    };
    if src.len() > MAX_SRC_BYTES {
        errors.lock().unwrap().push(diag::CompileError::new(format!(
            "input too large: {} bytes (limit {})", src.len(), MAX_SRC_BYTES)));
        return;
    }

    let jobs = collect_jobs(input, ctx.multi, &ctx.anchor_out, &ctx.cpp_out,
        &ctx.rust_out, &ctx.doc_out, &ctx.test_out);
    // 缓存签名覆盖所有期望产物（含 IR/SBOM 导出）
    let mut all_outs: Vec<PathBuf> = jobs.iter().map(|(_, p)| p.clone()).collect();
    if let Some(p) = &ctx.emit_ir { all_outs.push(p.clone()); }
    if let Some(p) = &ctx.sbom_out { all_outs.push(p.clone()); }

    // 增量缓存：解析“前”先查（仅按源文本哈希 + 版本 + 产物清单判断），
    // 命中则连解析/语义/生成都跳过，大 IDL 亦零开销。
    if !ctx.check_only && !ctx.no_cache && cache_hit(input, &src, &all_outs, &ctx.cache_dir) {
        eprintln!("[idlc] cache hit, skip {}", input.display());
        return;
    }

    // 解析 + 语义检查 + 构建 Bridge IR（仅 miss 时执行）
    let (ir_opt, perrs) = parse_and_check(&src, input);
    if !perrs.is_empty() { errors.lock().unwrap().extend(perrs); }
    let ir = match ir_opt { Some(ir) => Arc::new(ir), None => return };
    eprintln!("[idlc] parsed '{}' v{} ({} items), fingerprint=0x{:08x}",
              ir.module.name, ir.module.version, ir.module.items.len(), ir.fingerprint());
    eprintln!("[idlc] semantic OK; bridge IR v{}: {} layouts, {} opaque",
              ir.ir_version, ir.type_layouts.len(), ir.opaque_types.len());

    if let Some(p) = &ctx.emit_ir {
        if let Err(e) = emit_ir_file(p, &ir) { errors.lock().unwrap().push(e); }
    }

    // 并行生成各后端（后端相互独立）；panic 隔离：单后端 panic 转错误而非崩进程
    let produced: Mutex<Vec<(PathBuf, Option<String>)>> = Mutex::new(Vec::new());
    {
        let produced = &produced;
        let ir = &ir;
        std::thread::scope(|s| {
            for (kind, path) in &jobs {
                let ir = Arc::clone(ir);
                s.spawn(move || {
                    let code = panic::catch_unwind(AssertUnwindSafe(|| generate(*kind, &ir)));
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

    let mut sbom_files: Vec<(String, String)> = Vec::new();
    if ctx.sbom_out.is_some() { sbom_files.push((input.display().to_string(), src.clone())); }

    // 事务化写入：任一产物失败即回滚本批次
    let mut tx = WriteTx::new();
    let mut tx_failed = false;
    for (path, code) in ordered {
        let code = match code { Some(c) => c, None => continue };
        if ctx.check_only {
            match fs::read_to_string(&path) {
                Ok(existing) if existing == code =>
                    eprintln!("[idlc] ok (up-to-date) {}", path.display()),
                Ok(_) => errors.lock().unwrap().push(diag::CompileError::new(format!(
                    "{} is out of date; re-run xso-idlc without --check", path.display()))),
                Err(_) => errors.lock().unwrap().push(diag::CompileError::new(format!(
                    "{} is missing; re-run xso-idlc without --check", path.display()))),
            }
        } else if let Err(e) = tx.stage(&path, &code) {
            errors.lock().unwrap().push(e);
            tx_failed = true;
            break;
        }
        if ctx.sbom_out.is_some() { sbom_files.push((path.display().to_string(), code)); }
    }
    if tx_failed { tx.rollback(); return; }

    if let Some(p) = &ctx.sbom_out {
        let json = sbom::generate(GEN_VERSION, &ir, &sbom_files);
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() { let _ = fs::create_dir_all(parent); }
        }
        if let Err(e) = fs::write(p, json) {
            errors.lock().unwrap().push(diag::CompileError::new(
                format!("cannot write {}: {}", p.display(), e)));
        } else {
            eprintln!("[idlc] wrote SBOM {}", p.display());
        }
    }

    if !ctx.check_only && !ctx.no_cache { write_cache(input, &src, &all_outs, &ctx.cache_dir); }
}

fn generate(kind: Out, ir: &bridge_ir::BridgeIr) -> String {
    match kind {
        Out::Anchor => codegen::anchor::generate(ir),
        Out::Cpp => codegen::cpp::generate(ir),
        Out::Rust => codegen::rust::generate(ir),
        Out::Doc => codegen::doc::generate(ir),
        Out::Test => codegen::test::generate(ir),
    }
}

/// 解析 + 语义检查 + 构建 Bridge IR。带回退恢复，一次收集全部错误（含源码片段）。
fn parse_and_check(src: &str, input: &PathBuf) -> (Option<bridge_ir::BridgeIr>, Vec<diag::CompileError>) {
    let mut errs: Vec<diag::CompileError> = Vec::new();
    let (module_opt, mut perrs) = parser::parse_with_recovery(src);
    for e in perrs.drain(..) { errs.push(e.in_file(input.display().to_string())); }
    attach_source_lines(&mut errs, src);

    let mut module = match module_opt { Some(m) => m, None => return (None, errs) };
    match semantic::analyze(&mut module) {
        Ok(()) => {}
        Err(e) => { errs.push(e.in_file(input.display().to_string())); return (None, errs); }
    }
    (Some(bridge_ir::BridgeIr::build(&module)), errs)
}

fn attach_source_lines(errs: &mut Vec<diag::CompileError>, src: &str) {
    let lines: Vec<&str> = src.lines().collect();
    for e in errs.iter_mut() {
        if e.source_line.is_some() { continue; }
        if let Some(l) = e.line {
            if l >= 1 && l <= lines.len() { e.source_line = Some(lines[l - 1].to_string()); }
        }
    }
}

fn collect_jobs(
    input: &PathBuf, multi: bool,
    anchor_out: &Option<PathBuf>, cpp_out: &Option<PathBuf>,
    rust_out: &Option<PathBuf>, doc_out: &Option<PathBuf>, test_out: &Option<PathBuf>,
) -> Vec<(Out, PathBuf)> {
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".into());
    let dir = input.parent().filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let pick = |kind: Out, explicit: &Option<PathBuf>, suffix: &str| -> Option<(Out, PathBuf)> {
        if !multi { explicit.clone().map(|p| (kind, p)) }
        else { Some((kind, dir.join(format!("{}{}", stem, suffix)))) }
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
            eprintln!("[idlc] up-to-date {}", path.display());
            return Ok(());
        }
        let tmp = path.with_extension(format!("xso-tmp-{}", std::process::id()));
        fs::write(&tmp, code).map_err(|e| diag::CompileError::new(
            format!("cannot write {}: {}", tmp.display(), e)))?;
        fs::rename(&tmp, path).map_err(|e| diag::CompileError::new(
            format!("cannot rename {} -> {}: {}", tmp.display(), path.display(), e)))?;
        eprintln!("[idlc] wrote {}", path.display());
        Ok(())
    }

    /// 回滚：恢复旧内容；原先不存在的文件删除
    fn rollback(&self) {
        for (path, prev) in &self.entries {
            match prev {
                Some(old) => { let _ = fs::write(path, old); }
                None => { let _ = fs::remove_file(path); }
            }
        }
        eprintln!("[idlc] rolled back {} file(s)", self.entries.len());
    }
}

/// 导出版本化 IR（JSON）；若已存在则比对指纹给出 diff 结论
fn emit_ir_file(path: &PathBuf, ir: &bridge_ir::BridgeIr) -> diag::Result<()> {
    let json = ir.to_json();
    if let Ok(old) = fs::read_to_string(path) {
        let old_fp = extract_fingerprint(&old);
        let new_fp = Some(ir.fingerprint());
        if old_fp != new_fp {
            eprintln!("[idlc] ABI CHANGE: fingerprint {:?} -> {:#010x}", old_fp, ir.fingerprint());
        } else {
            eprintln!("[idlc] no ABI change (fingerprint {:#010x})", ir.fingerprint());
        }
        if old != json {
            let added = json.lines().filter(|l| !old.lines().any(|o| o == *l)).count();
            let removed = old.lines().filter(|l| !json.lines().any(|n| n == *l)).count();
            eprintln!("[idlc] IR diff: +{} -{} lines", added, removed);
        }
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() { let _ = fs::create_dir_all(parent); }
    }
    fs::write(path, json)
        .map_err(|e| diag::CompileError::new(format!("cannot write {}: {}", path.display(), e)))?;
    eprintln!("[idlc] wrote IR {}", path.display());
    Ok(())
}

pub(crate) fn extract_fingerprint(json: &str) -> Option<u32> {
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
pub(crate) fn run_migrate(path: &PathBuf) -> diag::Result<()> {
    let src = fs::read_to_string(path)
        .map_err(|e| diag::CompileError::new(format!("cannot read {}: {}", path.display(), e)))?;
    let m = migrate::migrate(&src);
    if !m.changed {
        eprintln!("[idlc] no migration needed: {}", path.display());
        return Ok(());
    }
    for r in &m.applied { eprintln!("[idlc] applied: {}", r); }
    fs::write(path, &m.content)
        .map_err(|e| diag::CompileError::new(format!("cannot write {}: {}", path.display(), e)))?;
    eprintln!("[idlc] migrated {}", path.display());
    Ok(())
}

// ===== 增量缓存（命中则跳过整轮解析+生成）=====
//
// 缓存键 = 源文本 FNV-1a 哈希 + IR 版本 + 生成器版本 + 期望产物清单。
// 生成器或输入任何变化都会令指纹改变 -> 自动失效。键名用输入路径哈希，
// 避免「同名模块不同文件」碰撞。

fn cache_path(dir: &Path, input: &Path) -> PathBuf {
    let h = fnv1a(&input.display().to_string());
    dir.join(format!("xso-{:016x}.cache", h))
}

fn cache_hit(input: &Path, src: &str, all_outs: &[PathBuf], dir: &Path) -> bool {
    let p = cache_path(dir, input);
    let Ok(text) = fs::read_to_string(&p) else { return false; };
    let mut in_hash = None;
    let mut ir_v = None;
    let mut gen_v = None;
    let mut outs: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("input_hash=") { in_hash = Some(v.to_string()); }
        else if let Some(v) = line.strip_prefix("ir_version=") { ir_v = Some(v.to_string()); }
        else if let Some(v) = line.strip_prefix("gen_version=") { gen_v = Some(v.to_string()); }
        else if let Some(v) = line.strip_prefix("out=") { outs.push(v.to_string()); }
    }
    if in_hash.as_deref() != Some(&format!("{:016x}", fnv1a(src))) { return false; }
    if ir_v.as_deref() != Some(&format!("{}", bridge_ir::IR_VERSION)) { return false; }
    if gen_v.as_deref() != Some(GEN_VERSION) { return false; }
    let cur: Vec<String> = all_outs.iter().map(|p| p.display().to_string()).collect();
    if cur != outs { return false; }
    outs.iter().all(|o| Path::new(o).exists())
}

fn write_cache(input: &Path, src: &str, all_outs: &[PathBuf], dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let mut s = String::new();
    s.push_str(&format!("input_hash={:016x}\n", fnv1a(src)));
    s.push_str(&format!("ir_version={}\n", bridge_ir::IR_VERSION));
    s.push_str(&format!("gen_version={}\n", GEN_VERSION));
    for p in all_outs { s.push_str(&format!("out={}\n", p.display())); }
    let _ = fs::write(cache_path(dir, input), s);
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}


