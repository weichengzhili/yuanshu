//! CLI 交互模式：无参数启动或 `--interactive` 时进入。
//!
//! 提供菜单驱动的引导式操作（生成 / 校验 / 查看 ABI / 迁移），
//! 内部复用与命令行完全一致的代码路径，保证交互与非交互行为一致。

use crate::diag::{CompileError, Result};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// 在任意交互输入处输入该词即请求退出整个交互模式（大小写不敏感）
const QUIT_TOKEN: &str = "__interactive_quit__";

/// 运行一个交互子命令。返回 true 表示用户在任意提示处输入 quit/exit/q 请求退出；
/// 其它错误会打印后继续，不会中断交互会话。
fn run_cmd(f: impl FnOnce() -> Result<()>) -> bool {
    match f() {
        Ok(()) => false,
        Err(e) if e.message == QUIT_TOKEN => { crate::out!("bye."); true }
        Err(e) => { crate::error!("{}", e); false }
    }
}

pub fn run_interactive() -> Result<()> {
    crate::out!("==============================================");
    crate::out!("  xso-idlc 交互模式 (IDL -> Rust / C++ / anchor)");
    crate::out!("==============================================");
    crate::out!("输入 'help' 或 '?' 查看可用命令；'quit' 或 'q' 退出。");
    loop {
        print!("xso-idlc> ");
        io::stdout().flush().ok();
        let mut line = String::new();
        let n = io::stdin().read_line(&mut line)
            .map_err(|e| crate::diag::CompileError::new(format!("read stdin failed: {}", e)))?;
        if n == 0 { break; } // EOF
        let line = line.trim();
        if line.is_empty() { continue; }
        let lower = line.to_ascii_lowercase();
        match lower.as_str() {
            "quit" | "exit" | "q" => { crate::out!("bye."); break; }
            "help" | "?" | "h" => { print_interactive_help(); continue; }
            "generate" | "gen" | "g" => { if run_cmd(do_generate) { break; } continue; }
            "check" | "k" => { if run_cmd(do_check) { break; } continue; }
            "ir" | "i" => { if run_cmd(do_ir) { break; } continue; }
            "migrate" | "mig" | "m" => { if run_cmd(do_migrate) { break; } continue; }
            "sbom" | "s" => { if run_cmd(do_sbom) { break; } continue; }
            "interactive" | "ui" => { crate::out!("already in interactive mode."); continue; }
            _ => { crate::out!("未知命令：'{}'（输入 'help' 查看可用命令）", line); continue; }
        }
    }
    Ok(())
}

/// 交互模式帮助：列出所有可用命令及其说明
fn print_interactive_help() {
    crate::out!("");
    crate::out!("可用命令：");
    crate::out!("  generate (gen, g)   生成 Rust / C++ / anchor 代码、文档与测试桩");
    crate::out!("  check    (k)        仅校验 IDL 语法与语义，不生成任何文件");
    crate::out!("  ir       (i)        查看中间表示 (IR)、共享结构体布局与 ABI 指纹");
    crate::out!("  migrate  (mig, m)    迁移旧版 IDL 语法到当前版本（支持原地覆盖）");
    crate::out!("  sbom     (s)        生成 SBOM（软件物料清单 JSON）");
    crate::out!("  help     (?, h)      显示本帮助");
    crate::out!("  quit     (exit, q)   退出交互模式");
    crate::out!("");
    crate::out!("说明：每条命令会逐项提示所需参数，直接回车可取消或采用默认值。");
}

/// 读取一行输入（空串表示直接回车；输入 quit/exit/q 请求退出整个交互模式）
fn prompt(msg: &str) -> Result<String> {
    print!("  {} > ", msg);
    std::io::stdout().flush().ok();
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)
        .map_err(|e| CompileError::new(format!("read stdin failed: {}", e)))?;
    let s = s.trim().to_string();
    if matches!(s.to_ascii_lowercase().as_str(), "quit" | "exit" | "q") {
        return Err(CompileError::new(QUIT_TOKEN));
    }
    Ok(s)
}

fn stem_of(p: &str) -> String {
    Path::new(p).file_stem().map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".into())
}

fn dir_of(p: &str) -> String {
    let path = Path::new(p);
    match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.display().to_string(),
        _ => ".".to_string(),
    }
}

fn join(dir: &str, file: &str) -> String {
    Path::new(dir).join(file).display().to_string()
}

/// 依据用户选择的后端拼装输出参数
fn build_args(input: &str, dir: &str, sel: &str, extra: &[String]) -> Vec<String> {
    let stem = stem_of(input);
    let all = sel.is_empty();
    let has = |c: char| all || sel.contains(c);
    let mut args = vec!["xso-idlc".to_string(), "--input".to_string(), input.to_string()];
    if has('a') {
        args.push("--anchor".into()); args.push(join(dir, &format!("{}.anchor.h", stem)));
    }
    if has('c') {
        args.push("--cpp-out".into()); args.push(join(dir, &format!("{}.h", stem)));
    }
    if has('r') {
        args.push("--rust-out".into()); args.push(join(dir, &format!("{}.rs", stem)));
    }
    if has('d') {
        args.push("--doc-out".into()); args.push(join(dir, &format!("{}.md", stem)));
    }
    if has('t') {
        args.push("--test-out".into()); args.push(join(dir, &format!("{}.test.rs", stem)));
    }
    for e in extra { args.push(e.clone()); }
    args
}

fn do_generate() -> Result<()> {
    let input = prompt("IDL 文件路径")?;
    if input.is_empty() { crate::out!("  已取消。"); return Ok(()); }
    if !Path::new(&input).exists() {
        return Err(CompileError::new(format!("文件不存在：{}", input)));
    }
    let dir_in = prompt("输出目录（回车=与输入同目录）")?;
    let dir = if dir_in.is_empty() { dir_of(&input) } else { dir_in };
    let sel = prompt("后端选择 a/c/r/d/t（回车=全部）")?;
    let args = build_args(&input, &dir, &sel, &[]);
    crate::run(&args)
}

fn do_check() -> Result<()> {
    let input = prompt("IDL 文件路径")?;
    if input.is_empty() { crate::out!("  已取消。"); return Ok(()); }
    let dir = dir_of(&input);
    let args = build_args(&input, &dir, "", &["--check".to_string()]);
    crate::run(&args)
}

fn do_ir() -> Result<()> {
    let input = prompt("IDL 文件路径")?;
    if input.is_empty() { crate::out!("  已取消。"); return Ok(()); }
    let src = std::fs::read_to_string(&input)
        .map_err(|e| CompileError::new(format!("cannot read {}: {}", input, e)))?;
    let (m, perrs) = crate::parser::parse_with_recovery(&src);
    if !perrs.is_empty() {
        for e in &perrs { crate::out!("  {}", e); }
        return Err(CompileError::new("解析失败，无法构建 IR"));
    }
    let mut m = m.ok_or_else(|| CompileError::new("解析失败"))?;
    crate::semantic::analyze(&mut m)?;
    let ir = crate::bridge_ir::BridgeIr::build(&m);
    crate::out!("");
    crate::out!("  模块        : {} v{}", ir.module.name, ir.module.version);
    crate::out!("  IR 版本     : {}", ir.ir_version);
    crate::out!("  ABI 指纹    : 0x{:08x}", ir.fingerprint());
    crate::out!("  共享结构体  : {} 个", ir.type_layouts.len());
    for (n, l) in &ir.type_layouts {
        crate::out!("    - {:<16} size={} align={}", n, l.size, l.align);
    }
    crate::out!("  枚举/错误   : {} 个", ir.enum_sizes.len());
    crate::out!("  不透明类型  : {:?}", ir.opaque_types.iter().collect::<Vec<_>>());
    Ok(())
}

fn do_migrate() -> Result<()> {
    let input = prompt("IDL 文件路径")?;
    if input.is_empty() { crate::out!("  已取消。"); return Ok(()); }
    let src = std::fs::read_to_string(&input)
        .map_err(|e| CompileError::new(format!("cannot read {}: {}", input, e)))?;
    let m = crate::migrate::migrate(&src);
    if !m.changed {
        crate::out!("  无需迁移：文件已是当前语法。");
        return Ok(());
    }
    crate::out!("  将应用以下规则：");
    for r in &m.applied { crate::out!("    - {}", r); }
    let out = prompt("输出路径（回车=原地覆盖）")?;
    let target: PathBuf = if out.is_empty() { PathBuf::from(&input) } else { PathBuf::from(out) };
    std::fs::write(&target, &m.content)
        .map_err(|e| CompileError::new(format!("cannot write {}: {}", target.display(), e)))?;
    crate::out!("  已写入 {}", target.display());
    Ok(())
}

fn do_sbom() -> Result<()> {
    let input = prompt("IDL 文件路径")?;
    if input.is_empty() { crate::out!("  已取消。"); return Ok(()); }
    let out = prompt("SBOM 输出路径（回车=<同目录>/<模块>.sbom.json）")?;
    let target = if out.is_empty() {
        join(&dir_of(&input), &format!("{}.sbom.json", stem_of(&input)))
    } else { out };
    let args = build_args(&input, &dir_of(&input), "",
        &["--sbom".to_string(), target]);
    crate::run(&args)
}
