//! 命令行子命令分发层。
//!
//! 形式：`xso-idlc <command> [options]`
//!   gen / check / ir / diff / migrate / sbom / doctor / interactive / version / help / completions
//!
//! 兼容旧式纯 flag 调用（以 `-` 开头）与无参数启动（进入交互模式）。

use crate::diag::{CompileError, Result};
use std::path::PathBuf;

/// 所有一级子命令
pub const COMMANDS: &[&str] = &[
    "gen", "check", "ir", "diff", "migrate", "sbom",
    "doctor", "interactive", "version", "help", "completions",
];

/// 通用选项（用于 completion 与 help）
pub const FLAGS: &[&str] = &[
    "--input", "--anchor", "--cpp-out", "--rust-out", "--doc-out", "--test-out",
    "--emit-ir", "--sbom", "--cache-dir", "--check", "--interactive",
    "--help", "--version",
];

pub fn dispatch(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return crate::interactive::run_interactive();
    }
    match args[1].as_str() {
        "gen" => cmd_gen(&args[2..]),
        "check" => cmd_check(&args[2..]),
        "ir" => cmd_ir(&args[2..]),
        "diff" => cmd_diff(&args[2..]),
        "migrate" => cmd_migrate(&args[2..]),
        "sbom" => cmd_sbom(&args[2..]),
        "doctor" => cmd_doctor(),
        "interactive" | "ui" => crate::interactive::run_interactive(),
        "version" | "--version" | "-V" => {
            println!("xso-idlc {} (ir {})", crate::GEN_VERSION, crate::bridge_ir::IR_VERSION);
            Ok(())
        }
        "help" | "--help" | "-h" => { print_help(); Ok(()) }
        "completions" => cmd_completions(&args[2..]),
        // 兼容旧式：xso-idlc --input a.xso --rust-out a.rs
        other if other.starts_with('-') => crate::run(args),
        other => Err(CompileError::new(
            format!("unknown command '{}'；可用：{}（或 `xso-idlc help`）", other, COMMANDS.join(", ")))),
    }
}

// ===================== 参数工具 =====================

struct Opts {
    inputs: Vec<PathBuf>,
    map: std::collections::HashMap<String, String>,
}

impl Opts {
    fn parse(args: &[String]) -> Result<Self> {
        let mut inputs = Vec::new();
        let mut map = std::collections::HashMap::new();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--input" | "-i" => { inputs.push(PathBuf::from(&args[i + 1])); i += 2; }
                "--out" | "-o" => { map.insert("--out".into(), args[i + 1].clone()); i += 2; }
                "--base" => { map.insert("--base".into(), args[i + 1].clone()); i += 2; }
                "--json" => { map.insert("--json".into(), args[i + 1].clone()); i += 2; }
                "--cache-dir" => { map.insert("--cache-dir".into(), args[i + 1].clone()); i += 2; }
                other if other.starts_with("--") => {
                    // 允许 --k=v 与 --k v
                    if let Some((k, v)) = other.split_once('=') {
                        map.insert(k.to_string(), v.to_string());
                        i += 1;
                    } else {
                        if i + 1 >= args.len() {
                            return Err(CompileError::new(format!("missing value for {}", other)));
                        }
                        map.insert(other.to_string(), args[i + 1].clone());
                        i += 2;
                    }
                }
                other => return Err(CompileError::new(format!("unexpected argument '{}'", other))),
            }
        }
        Ok(Opts { inputs, map })
    }
    fn get(&self, k: &str) -> Option<&String> { self.map.get(k) }
}

/// 由子命令选项拼出旧式 argv，复用既有 `run` 逻辑
fn to_argv(inputs: &[PathBuf], extra: &[(&str, Option<String>)], inject: &[&str]) -> Vec<String> {
    let mut argv = vec!["xso-idlc".to_string()];
    for p in inputs {
        argv.push("--input".into());
        argv.push(p.display().to_string());
    }
    for (k, v) in extra {
        if let Some(v) = v {
            argv.push((*k).to_string());
            argv.push(v.clone());
        }
    }
    for f in inject { argv.push((*f).to_string()); }
    argv
}

// ===================== 子命令 =====================

fn cmd_gen(args: &[String]) -> Result<()> {
    let o = Opts::parse(args)?;
    if o.inputs.is_empty() { return Err(CompileError::new("gen: missing --input <file.xso>")); }
    let argv = to_argv(&o.inputs, &[
        ("--anchor", o.get("--anchor").cloned()),
        ("--cpp-out", o.get("--cpp-out").cloned()),
        ("--rust-out", o.get("--rust-out").cloned()),
        ("--doc-out", o.get("--doc-out").cloned()),
        ("--test-out", o.get("--test-out").cloned()),
        ("--emit-ir", o.get("--emit-ir").cloned()),
        ("--sbom", o.get("--sbom").cloned()),
        ("--cache-dir", o.get("--cache-dir").cloned()),
    ], &[]);
    crate::run(&argv)
}

fn cmd_check(args: &[String]) -> Result<()> {
    let o = Opts::parse(args)?;
    if o.inputs.is_empty() { return Err(CompileError::new("check: missing --input <file.xso>")); }
    let argv = to_argv(&o.inputs, &[
        ("--anchor", o.get("--anchor").cloned()),
        ("--cpp-out", o.get("--cpp-out").cloned()),
        ("--rust-out", o.get("--rust-out").cloned()),
        ("--doc-out", o.get("--doc-out").cloned()),
        ("--test-out", o.get("--test-out").cloned()),
    ], &["--check"]);
    crate::run(&argv)
}

fn cmd_ir(args: &[String]) -> Result<()> {
    let o = Opts::parse(args)?;
    if o.inputs.is_empty() { return Err(CompileError::new("ir: missing --input <file.xso>")); }
    let mut all_ok = true;
    for input in &o.inputs {
        let src = std::fs::read_to_string(input).map_err(|e| CompileError::new(
            format!("cannot read {}: {}", input.display(), e)))?;
        let (m, perrs) = crate::parser::parse_with_recovery(&src);
        if !perrs.is_empty() {
            all_ok = false;
            for e in &perrs { eprintln!("{}", e); }
            continue;
        }
        let mut m = m.ok_or_else(|| CompileError::new("parse failed"))?;
        crate::semantic::analyze(&mut m)?;
        let ir = crate::bridge_ir::BridgeIr::build(&m);
        println!("--- {} ---", input.display());
        println!("module      : {} v{}", ir.module.name, ir.module.version);
        println!("ir version  : {}", ir.ir_version);
        println!("fingerprint : 0x{:08x}", ir.fingerprint());
        println!("layouts     : {} (structs), {} (enums/errors)",
                 ir.type_layouts.len(), ir.enum_sizes.len());
        for (n, l) in &ir.type_layouts {
            println!("  struct {:<12} size={:<4} align={}", n, l.size, l.align);
        }
        if let Some(path) = o.get("--json") {
            std::fs::write(path, ir.to_json()).map_err(|e| CompileError::new(
                format!("cannot write {}: {}", path, e)))?;
            println!("IR JSON     : -> {}", path);
        }
    }
    if all_ok { Ok(()) } else { Err(CompileError::new("ir: some inputs failed")) }
}

/// ABI 差异：与基线比对（基线可为 .xso 或已导出 IR 的 .json）
fn cmd_diff(args: &[String]) -> Result<()> {
    let o = Opts::parse(args)?;
    let base = o.get("--base").cloned()
        .ok_or_else(|| CompileError::new("diff: missing --base <file.xso|ir.json>"))?;
    if o.inputs.is_empty() { return Err(CompileError::new("diff: missing --input <file.xso>")); }

    let current = build_ir(&o.inputs[0])?;

    if base.ends_with(".json") {
        let old = std::fs::read_to_string(&base).map_err(|e| CompileError::new(
            format!("cannot read {}: {}", base, e)))?;
        let old_fp = crate::extract_fingerprint(&old);
        println!("base fingerprint  : {:?}", old_fp.map(|f| format!("0x{:08x}", f)));
        println!("current fingerprint: 0x{:08x}", current.fingerprint());
        return match old_fp {
            Some(f) if f == current.fingerprint() => { println!("[none] no ABI change"); Ok(()) }
            _ => { println!("[break] ABI fingerprint changed"); Ok(()) }
        };
    }

    let baseline = build_ir(&PathBuf::from(&base))?;
    let changes = baseline.diff(&current);
    if changes.is_empty() { println!("[none] no ABI change"); return Ok(()); }
    for c in &changes { println!("{}", c); }
    if changes.iter().any(|c| c.starts_with("[break]")) {
        return Err(CompileError::new("diff: breaking ABI change detected"));
    }
    Ok(())
}

fn build_ir(path: &PathBuf) -> Result<crate::bridge_ir::BridgeIr> {
    let src = std::fs::read_to_string(path).map_err(|e| CompileError::new(
        format!("cannot read {}: {}", path.display(), e)))?;
    let (m, perrs) = crate::parser::parse_with_recovery(&src);
    if !perrs.is_empty() {
        return Err(CompileError::new(format!("{}: {} parse error(s)", path.display(), perrs.len())));
    }
    let mut m = m.ok_or_else(|| CompileError::new("parse failed"))?;
    crate::semantic::analyze(&mut m)?;
    Ok(crate::bridge_ir::BridgeIr::build(&m))
}

fn cmd_migrate(args: &[String]) -> Result<()> {
    let o = Opts::parse(args)?;
    if o.inputs.is_empty() { return Err(CompileError::new("migrate: missing --input <file.xso>")); }
    for p in &o.inputs { crate::run_migrate(p)?; }
    Ok(())
}

fn cmd_sbom(args: &[String]) -> Result<()> {
    let o = Opts::parse(args)?;
    if o.inputs.is_empty() { return Err(CompileError::new("sbom: missing --input <file.xso>")); }
    let out = o.get("--out").or_else(|| o.get("--sbom")).cloned()
        .ok_or_else(|| CompileError::new("sbom: missing --out <file.json>"))?;
    let argv = to_argv(&o.inputs, &[
        ("--anchor", o.get("--anchor").cloned()),
        ("--cpp-out", o.get("--cpp-out").cloned()),
        ("--rust-out", o.get("--rust-out").cloned()),
        ("--sbom", Some(out)),
    ], &[]);
    crate::run(&argv)
}

/// 自检：示例可解析、生成确定且幂等、缓存目录可写
fn cmd_doctor() -> Result<()> {
    println!("xso-idlc doctor");
    println!("  generator : {}", crate::GEN_VERSION);
    println!("  ir version: {}", crate::bridge_ir::IR_VERSION);
    println!();

    let cases: [(&str, &str); 3] = [
        ("fs", include_str!("examples/fs.xso")),
        ("net", include_str!("examples/net.xso")),
        ("kv", include_str!("examples/kv.xso")),
    ];

    let mut failures = 0;
    for (tag, src) in cases {
        let (m, perrs) = crate::parser::parse_with_recovery(src);
        if !perrs.is_empty() || m.is_none() {
            println!("  [FAIL] {}: parse errors", tag);
            failures += 1;
            continue;
        }
        let mut m = m.unwrap();
        if let Err(e) = crate::semantic::analyze(&mut m) {
            println!("  [FAIL] {}: semantic: {}", tag, e);
            failures += 1;
            continue;
        }
        let ir = crate::bridge_ir::BridgeIr::build(&m);
        // 幂等性：连续两次生成必须完全一致
        let a = crate::codegen::rust::generate(&ir);
        let b = crate::codegen::rust::generate(&ir);
        let c = crate::codegen::cpp::generate(&ir);
        let d = crate::codegen::cpp::generate(&ir);
        if a != b || c != d {
            println!("  [FAIL] {}: generation is not deterministic", tag);
            failures += 1;
            continue;
        }
        println!("  [ok] {:<4} fingerprint=0x{:08x} rust={}B cpp={}B",
                 tag, ir.fingerprint(), a.len(), c.len());
    }

    // 缓存目录可写性
    let cache = std::env::temp_dir().join("xso-idlc-doctor");
    match std::fs::create_dir_all(&cache) {
        Ok(()) => println!("  [ok] cache dir writable: {}", cache.display()),
        Err(e) => { println!("  [FAIL] cache dir: {}", e); failures += 1; }
    }

    println!();
    if failures > 0 {
        Err(CompileError::new(format!("doctor: {} check(s) failed", failures)))
    } else {
        println!("  all checks passed");
        Ok(())
    }
}

fn cmd_completions(args: &[String]) -> Result<()> {
    let shell = args.first().cloned().unwrap_or_else(|| "bash".into());
    let cmds = COMMANDS.join(" ");
    let flags = FLAGS.join(" ");
    let script = match shell.as_str() {
        "bash" => format!(
            "_xso_idlc() {{\n  local cur=\"${{COMP_WORDS[COMP_CWORD]}}\"\n  \
             COMPREPLY=( $(compgen -W \"{} {}\" -- \"$cur\") )\n}}\n\
             complete -F _xso_idlc xso-idlc\n", cmds, flags),
        "zsh" => format!(
            "#compdef xso-idlc\n_xso_idlc() {{\n  local -a cmds flags\n  \
             cmds=({})\n  flags=({})\n  _arguments '1: :(${{cmds}})' '*: :(${{flags}})'\n}}\n\
             compdef _xso_idlc xso-idlc\n", cmds, flags),
        "fish" => COMMANDS.iter()
            .map(|c| format!("complete -c xso-idlc -n '__fish_use_subcommand' -a '{}'", c))
            .collect::<Vec<_>>().join("\n") + "\n",
        "powershell" => format!(
            "Register-ArgumentCompleter -Native -CommandName xso-idlc -ScriptBlock {{\n  \
             param($wordToComplete)\n  @({}) | Where-Object {{ $_ -like \"$wordToComplete*\" }}\n}}\n",
            COMMANDS.iter().map(|c| format!("'{}'", c)).collect::<Vec<_>>().join(",")),
        other => return Err(CompileError::new(
            format!("unsupported shell '{}' (bash|zsh|fish|powershell)", other))),
    };
    print!("{}", script);
    Ok(())
}

pub fn print_help() {
    println!("xso-idlc {} — XuanShu IDL Compiler", crate::GEN_VERSION);
    println!();
    println!("USAGE:");
    println!("    xso-idlc <command> [options]");
    println!("    xso-idlc --input <file.xso> [options]   (legacy flag form)");
    println!("    xso-idlc                                (interactive mode)");
    println!();
    println!("COMMANDS:");
    println!("    gen          Generate Rust/C++/anchor/doc/test from IDL");
    println!("    check        Verify generated outputs are up to date (CI gate, no writes)");
    println!("    ir           Show ABI fingerprint / IR summary (--json to export)");
    println!("    diff         ABI diff against a baseline (--base <file.xso|ir.json>)");
    println!("    migrate      Upgrade legacy IDL syntax in place (idempotent)");
    println!("    sbom         Write CycloneDX SBOM with SHA-256 of artifacts");
    println!("    doctor       Self-check: examples parse, generation deterministic");
    println!("    interactive  Menu-driven mode (alias: ui)");
    println!("    completions  Shell completions (bash|zsh|fish|powershell)");
    println!("    version      Print versions");
    println!("    help         This help");
    println!();
    println!("OPTIONS (gen/check):");
    println!("    --input <path>     IDL input (repeatable)");
    println!("    --anchor <path>    Layout anchor header");
    println!("    --cpp-out <path>   C++ header");
    println!("    --rust-out <path>  Rust module");
    println!("    --doc-out <path>   Markdown doc");
    println!("    --test-out <path>  Layout test scaffold");
    println!("    --emit-ir <path>   Export versioned IR as JSON (+ ABI diff)");
    println!("    --sbom <path>      Write SBOM");
    println!("    --cache-dir <path> Incremental cache dir (default .xso-cache)");
    println!();
    println!("EXAMPLES:");
    println!("    xso-idlc gen --input src/examples/fs.xso --rust-out build/fs_gen.rs");
    println!("    xso-idlc ir --input src/examples/net.xso");
    println!("    xso-idlc diff --input new.xso --base old.xso");
    println!("    xso-idlc check --input src/examples/kv.xso");
}

fn print_command_help(name: &str) {
    match find_doc(name) {
        Some(d) => {
            println!("NAME");
            println!("    xso-idlc {} - {}", d.name, d.summary);
            println!();
            println!("SYNOPSIS");
            println!("    {}", d.usage);
            println!();
            println!("DESCRIPTION");
            for line in d.description.lines() { println!("    {}", line); }
            println!();
            println!("OPTIONS");
            for o in d.options { println!("    {}", o); }
            if !d.examples.is_empty() {
                println!();
                println!("EXAMPLES");
                for e in d.examples { println!("    {}", e); }
            }
            println!();
            println!("SEE ALSO");
            for s in d.see_also { println!("    {}", s); }
        }
        None => {
            eprintln!("unknown command: {}", name);
            print_general_help();
        }
    }
}

fn cmd_help(args: &[String]) {
    if args.is_empty() { print_general_help(); }
    else { print_command_help(&args[0]); }
}
