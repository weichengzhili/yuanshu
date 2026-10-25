mod ast;
mod bridge_ir;
mod codegen;
mod diag;
mod lexer;
mod parser;
mod semantic;
fn print_help() {
    println!("xso-idlc {} — XuanShu IDL Compiler", GEN_VERSION);
    println!();
    println!("USAGE:");
    println!("    xso-idlc --input <file.xso> [--input <more.xso> ...] [options]");
    println!();
    println!("OPTIONS:");
    println!("    --input     <path>  Input IDL (repeatable; built in parallel)");
    println!("    --anchor    <path>  Layout anchor header");
    println!("    --cpp-out   <path>  C++ header");
    println!("    --rust-out  <path>  Rust module");
    println!("    --doc-out   <path>  Markdown doc");
    println!("    --test-out  <path>  Layout test scaffold");
    println!("    --emit-ir   <path>  Export versioned IR as JSON (+ ABI diff)");
    println!("    --sbom      <path>  Write CycloneDX SBOM with SHA-256 of artifacts");
    println!("    --cache-dir <path>  Incremental cache dir (default .xso-cache)");
    println!("    --jobs <N>           Max parallel build threads (default: all cores)");
    println!("    --no-cache           Disable incremental cache (force full rebuild)");
    println!("    --migrate  <path>    Upgrade legacy IDL syntax in place (idempotent)");
    println!("    --check              Verify outputs are up to date (CI gate, no writes)");
    println!("    --interactive, -i    Interactive menu mode");
    println!("    --help, -h           This help");
    println!("    --version, -V       Version");
    println!();
    println!("NOTES:");
    println!("    Multiple --input are parsed, generated and written in parallel.");
    println!("    Incremental cache skips re-parse + re-generate when inputs are unchanged.");
}
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

/// 生成器版本（增量/缓存失效）
const GEN_VERSION: &str = "3";
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
            // 集中错误输出：所有错误已在上游聚合，这里只负责打印
            eprintln!("{}", e);
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> diag::Result<()> {
    if args.len() <= 1 { return interactive::run_interactive(); }

    let mut input = None;
    let mut anchor_out = None;
    let mut cpp_out = None;
    let mut rust_out = None;
    let mut doc_out = None;
    let mut test_out = None;
    let mut emit_ir: Option<PathBuf> = None;
    let mut cache_dir = PathBuf::from(".xso-cache");
    let mut check_only = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--input"    => { input      = Some(PathBuf::from(&args[i+1])); i += 2; }
            "--anchor"   => { anchor_out = Some(PathBuf::from(&args[i+1])); i += 2; }
            "--cpp-out"  => { cpp_out    = Some(PathBuf::from(&args[i+1])); i += 2; }
            "--rust-out" => { rust_out   = Some(PathBuf::from(&args[i+1])); i += 2; }
            "--doc-out"  => { doc_out    = Some(PathBuf::from(&args[i+1])); i += 2; }
            "--test-out" => { test_out   = Some(PathBuf::from(&args[i+1])); i += 2; }
            "--emit-ir" => { emit_ir = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "--cache-dir" => { cache_dir = PathBuf::from(&args[i + 1])); i += 2; }
            "--check" => { check_only = true; i += 1; }
            "--help" | "-h" => { print_help(); return Ok(()); }
            "--version" | "-V" => { println!("xso-idlc {}", GEN_VERSION); return Ok(()); }
            other => return Err(diag::CompileError::new(format!("unknown argument: {}", other))),
        }
    }

    // 多输入时，单文件输出参数失效，改为按输入名推导；单输入时优先使用给定输出路径。
    if interactive_flag { return interactive::run_interactive(); }
    // 交互模式
    if inputs.is_empty() {
        return Err(diag::CompileError::new("missing --input <file.xso>"));
    }

    // 迁移模式：改写旧 IDL 语法后直接返回
    if let Some(p) = &migrate_path {
        return run_migrate(p);
    }

    let multi = inputs.len() > 1;
    let errors: Mutex<Vec<diag::CompileError>> = Mutex::new(Vec::new());

    for input in &inputs {
        let (ir_opt, perrs) = parse_and_check(input);
        if !perrs.is_empty() { errors.lock().unwrap().extend(perrs); }
        let ir = match ir_opt {
            Some(ir) => Arc::new(ir),
            None => continue,
        };
        eprintln!("[idlc] parsed '{}' v{} ({} items), fingerprint=0x{:08x}",
                  ir.module.name, ir.module.version, ir.module.items.len(), ir.fingerprint());
        eprintln!("[idlc] semantic OK; bridge IR v{}: {} layouts, {} opaque",
                  ir.ir_version, ir.type_layouts.len(), ir.opaque_types.len());

    let errors: Mutex<Vec<diag::CompileError>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for chunk in inputs.chunks(per) {
            let errors = &errors;
            s.spawn(move || {
                for input in chunk { process_input(input, &ctx, errors); }
            });
        }

    let errs = errors.into_inner().unwrap();
    if !errs.is_empty() {
        eprintln!("[idlc] {} error(s):", errs.len());
        for e in &errs {
            eprintln!("  {}", e);
        }
        return Err(diag::CompileError::new("code generation failed; see errors above"));
    }
    Ok(())
    if let Some(path) = doc_out {
        let code = codegen::doc::generate(&ir);
        write_out(&path, &code)?;
    }
    if let Some(path) = test_out {
        let code = codegen::test::generate(&ir);
        write_out(&path, &code)?;
    }

        // 增量缓存：输入哈希 + 版本一致且产物齐全 -> 整轮跳过
        if !check_only && cache_hit(input, &ir, &jobs, &cache_dir) {
            eprintln!("[idlc] cache hit, skipping generation for {}", input.display());
            continue;
        }

        // 多线程生成（后端相互独立）；panic 隔离：单后端 panic 转为错误而非崩进程
        let produced: Mutex<Vec<(PathBuf, Option<String>)>> = Mutex::new(Vec::new());
        {
            let errors = &errors;
            let produced = &produced;
            std::thread::scope(|s| {
                for (kind, path) in &jobs {
                    let ir = Arc::clone(&ir);
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
        if sbom_out.is_some() {
            if let Ok(src) = fs::read_to_string(input) {
                sbom_files.push((input.display().to_string(), src));
            }
        }

    if let Some(p) = &ctx.emit_ir {
        if let Err(e) = emit_ir_file(p, &ir) { errors.lock().unwrap().push(e); }
    }

    // 并行生成各后端（后端相互独立）；panic 隔离：单后端 panic 转错误而非崩进程
    let produced: Mutex<Vec<(PathBuf, Option<String>)>> = Mutex::new(Vec::new());
    {
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
            if sbom_out.is_some() {
                sbom_files.push((path.display().to_string(), code));
            }
        }
        if tx_failed {
            tx.rollback();
            continue;
        }

        // SBOM：产物清单 + SHA-256（供应链审计 / 完整性追踪）
        if let Some(p) = &sbom_out {
            let json = sbom::generate(GEN_VERSION, &ir, &sbom_files);
            if let Some(parent) = p.parent() {
                if !parent.as_os_str().is_empty() { let _ = fs::create_dir_all(parent); }
            }
            match fs::write(p, json) {
                Ok(()) => eprintln!("[idlc] wrote SBOM {}", p.display()),
                Err(e) => errors.lock().unwrap().push(diag::CompileError::new(
                    format!("cannot write {}: {}", p.display(), e))),
            }
        }

        if !check_only { write_cache(input, &ir, &jobs, &cache_dir); }
    }

    let errs = errors.into_inner().unwrap();
    if !errs.is_empty() {
        eprintln!("[idlc] {} error(s):", errs.len());
        for e in &errs { eprintln!("  {}", e); }
        return Err(diag::CompileError::new("generation failed; see errors above"));
    }
    Ok(())
}

/// 解析 + 语义检查 + 构建 Bridge IR。
/// 带回退恢复：一次收集全部语法/语义错误，每条都带文件、行列与源码片段。
fn parse_and_check(input: &PathBuf) -> (Option<bridge_ir::BridgeIr>, Vec<diag::CompileError>) {
    let mut errs: Vec<diag::CompileError> = Vec::new();
    let src = match fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => return (None, vec![diag::CompileError::new(
            format!("cannot read {}: {}", input.display(), e))]),
    };
    // 资源限制：源文件大小
    if src.len() > MAX_SRC_BYTES {
        return (None, vec![diag::CompileError::new(format!(
            "input too large: {} bytes (limit {})", src.len(), MAX_SRC_BYTES))]);
    }

    let (module_opt, mut perrs) = parser::parse_with_recovery(&src);
    for e in perrs.drain(..) {
        errs.push(e.in_file(input.display().to_string()));
    }
    attach_source_lines(&mut errs, &src);

    // 解析：错误恢复模式，收集全部语法错误
    let module = match module_opt { Some(m) => m, None => return (None, errs) };
    match semantic::analyze(&module) {
        Ok(()) => {}
        Err(e) => { errs.push(e.in_file(input.display().to_string())); return (None, errs); }
    }
    (Some(bridge_ir::BridgeIr::build(&module)), errs)
}

/// 为每条错误附上出错行的源码文本，便于渲染 caret 指示
fn attach_source_lines(errs: &mut Vec<diag::CompileError>, src: &str) {
    let lines: Vec<&str> = src.lines().collect();
    for e in errs.iter_mut() {
        if e.source_line.is_some() { continue; }
        if let Some(l) = e.line {
            if l >= 1 && l <= lines.len() {
                e.source_line = Some(lines[l - 1].to_string());
            }
        }
        // 原子写：先落临时文件再 rename，避免中途失败留下半截文件
    }
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

    let mut pick = |kind: Out, explicit: &Option<PathBuf>, suffix: &str| -> Option<(Out, PathBuf)> {
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
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == code { eprintln!("[idlc] up-to-date {}", path.display()); return Ok(()); }
    }
    fs::write(path, code)
        .map_err(|e| diag::CompileError::new(format!("cannot write {}: {}", path.display(), e)))?;
    eprintln!("[idlc] wrote {}", path.display());
    Ok(())
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
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    fs::write(path, json)
        .map_err(|e| diag::CompileError::new(format!("cannot write {}: {}", path.display(), e)))?;
    eprintln!("[idlc] wrote IR {}", path.display());
    Ok(())
}

fn extract_fingerprint(json: &str) -> Option<u32> {
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
fn run_migrate(path: &PathBuf) -> diag::Result<()> {
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

// ===== 缓存（增量编译）=====

fn cache_path(dir: &Path, module: &str) -> PathBuf {
    dir.join(format!("{}.cache", module))
}

fn cache_hit(input: &Path, ir: &bridge_ir::BridgeIr, jobs: &[(Out, PathBuf)], dir: &Path) -> bool {
    let p = cache_path(dir, &ir.module.name);
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
    let Ok(src) = fs::read_to_string(input) else { return false; };
    let want = format!("{:016x}", fnv1a(&src));
    if in_hash.as_deref() != Some(want.as_str()) { return false; }
    if ir_v.as_deref() != Some(format!("{}", ir.ir_version).as_str()) { return false; }
    if gen_v.as_deref() != Some(GEN_VERSION) { return false; }
    // 产物路径集合一致且均存在
    let cur: Vec<String> = jobs.iter().map(|(_, p)| p.display().to_string()).collect();
    if cur != outs { return false; }
    outs.iter().all(|o| Path::new(o).exists())
}

fn write_cache(input: &Path, ir: &bridge_ir::BridgeIr, jobs: &[(Out, PathBuf)], dir: &Path) {
    let Ok(src) = fs::read_to_string(input) else { return; };
    let _ = fs::create_dir_all(dir);
    let mut s = String::new();
    s.push_str(&format!("input_hash={:016x}\n", fnv1a(&src)));
    s.push_str(&format!("ir_version={}\n", ir.ir_version));
    s.push_str(&format!("gen_version={}\n", GEN_VERSION));
    for (_, p) in jobs { s.push_str(&format!("out={}\n", p.display())); }
    let _ = fs::write(cache_path(dir, &ir.module.name), s);
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}


