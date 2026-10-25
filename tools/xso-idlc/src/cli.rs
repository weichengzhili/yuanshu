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

/// 子命令参数里是否含帮助标志
fn has_help_flag(args: &[String]) -> bool {
    args.iter().any(|a| a == "--help" || a == "-h")
}

pub fn dispatch(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return crate::interactive::run_interactive();
    }
    let sub = args[1].as_str();
    let rest = &args[2..];

    match sub {
        "help" => return cmd_help(rest),
        "version" | "--version" | "-V" => {
            crate::out!("xso-idlc {} (ir {})", crate::GEN_VERSION, crate::bridge_ir::IR_VERSION);
            return Ok(());
        }
        "--help" | "-h" => { print_general_help(); return Ok(()); }
        "completions" => return cmd_completions(rest),
        _ => {}
    }

    // 子命令级 --help：显示该命令的详细帮助
    if has_help_flag(rest) {
        return cmd_help(&[sub.to_string()]);
    }

    match sub {
        "gen" => cmd_gen(rest),
        "check" => cmd_check(rest),
        "ir" => cmd_ir(rest),
        "diff" => cmd_diff(rest),
        "migrate" => cmd_migrate(rest),
        "sbom" => cmd_sbom(rest),
        "doctor" => cmd_doctor(),
        "interactive" | "ui" => crate::interactive::run_interactive(),
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
        // 取值助手：越界即报错（而非 panic），实现「选项缺取值」校验
        let take = |i: &mut usize, flag: &str| -> Result<String> {
            if *i + 1 >= args.len() {
                return Err(CompileError::new(format!("missing value for {}", flag)));
            }
            let v = args[*i + 1].clone();
            *i += 2;
            Ok(v)
        };
        while i < args.len() {
            match args[i].as_str() {
                "--input" | "-i" => { inputs.push(PathBuf::from(take(&mut i, "--input")?)); }
                "--out" | "-o" => { map.insert("--out".into(), take(&mut i, "--out")?); }
                "--base" => { map.insert("--base".into(), take(&mut i, "--base")?); }
                "--json" => { map.insert("--json".into(), take(&mut i, "--json")?); }
                "--cache-dir" => { map.insert("--cache-dir".into(), take(&mut i, "--cache-dir")?); }
                other if other.starts_with("--") => {
                    // 允许 --k=v 与 --k v
                    if let Some((k, v)) = other.split_once('=') {
                        map.insert(k.to_string(), v.to_string());
                        i += 1;
                    } else {
                        map.insert(other.to_string(), take(&mut i, other)?);
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
            for e in &perrs { crate::error!("{}", e); }
            continue;
        }
        let mut m = m.ok_or_else(|| CompileError::new("parse failed"))?;
        crate::semantic::analyze(&mut m)?;
        let ir = crate::bridge_ir::BridgeIr::build(&m);
        crate::out!("--- {} ---", input.display());
        crate::out!("module      : {} v{}", ir.module.name, ir.module.version);
        crate::out!("ir version  : {}", ir.ir_version);
        crate::out!("fingerprint : 0x{:08x}", ir.fingerprint());
        crate::out!("layouts     : {} (structs), {} (enums/errors)",
                 ir.type_layouts.len(), ir.enum_sizes.len());
        for (n, l) in &ir.type_layouts {
            crate::out!("  struct {:<12} size={:<4} align={}", n, l.size, l.align);
        }
        if let Some(path) = o.get("--json") {
            std::fs::write(path, ir.to_json()).map_err(|e| CompileError::new(
                format!("cannot write {}: {}", path, e)))?;
            crate::out!("IR JSON     : -> {}", path);
        }
        if let Some(path) = o.get("--bin") {
            std::fs::write(path, ir.to_bin()).map_err(|e| CompileError::new(
                format!("cannot write {}: {}", path, e)))?;
            crate::out!("IR binary   : -> {} ({} bytes)", path, ir.to_bin().len());
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
        crate::out!("base fingerprint  : {:?}", old_fp.map(|f| format!("0x{:08x}", f)));
        crate::out!("current fingerprint: 0x{:08x}", current.fingerprint());
        return match old_fp {
            Some(f) if f == current.fingerprint() => { crate::out!("[none] no ABI change"); Ok(()) }
            _ => { crate::out!("[break] ABI fingerprint changed"); Ok(()) }
        };
    }

    if base.ends_with(".bin") {
        // 离线 IR（二进制）反序列化后做真实 diff
        let bytes = std::fs::read(&base).map_err(|e| CompileError::new(
            format!("cannot read {}: {}", base, e)))?;
        let baseline = crate::bridge_ir::BridgeIr::from_bin(&bytes)
            .map_err(|e| CompileError::new(format!("bad IR binary {}: {}", base, e)))?;
        let changes = baseline.diff(&current);
        if changes.is_empty() { crate::out!("[none] no ABI change"); return Ok(()); }
        for c in &changes { crate::out!("{}", c); }
        return if changes.iter().any(|c| c.starts_with("[break]")) {
            Err(CompileError::new("diff: breaking ABI change detected"))
        } else { Ok(()) };
    }

    let baseline = build_ir(&PathBuf::from(&base))?;
    let changes = baseline.diff(&current);
    if changes.is_empty() { crate::out!("[none] no ABI change"); return Ok(()); }
    for c in &changes { crate::out!("{}", c); }
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
    crate::out!("xso-idlc doctor");
    crate::out!("  generator : {}", crate::GEN_VERSION);
    crate::out!("  ir version: {}", crate::bridge_ir::IR_VERSION);
    crate::out!("");

    let cases: [(&str, &str); 3] = [
        ("fs", include_str!("examples/fs.xso")),
        ("net", include_str!("examples/net.xso")),
        ("kv", include_str!("examples/kv.xso")),
    ];

    let mut failures = 0;
    for (tag, src) in cases {
        let (m, perrs) = crate::parser::parse_with_recovery(src);
        if !perrs.is_empty() || m.is_none() {
            crate::out!("  [FAIL] {}: parse errors", tag);
            failures += 1;
            continue;
        }
        let mut m = m.unwrap();
        if let Err(e) = crate::semantic::analyze(&mut m) {
            crate::out!("  [FAIL] {}: semantic: {}", tag, e);
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
            crate::out!("  [FAIL] {}: generation is not deterministic", tag);
            failures += 1;
            continue;
        }
        crate::out!("  [ok] {:<4} fingerprint=0x{:08x} rust={}B cpp={}B",
                 tag, ir.fingerprint(), a.len(), c.len());
    }

    // 缓存目录可写性
    let cache = std::env::temp_dir().join("xso-idlc-doctor");
    match std::fs::create_dir_all(&cache) {
        Ok(()) => crate::out!("  [ok] cache dir writable: {}", cache.display()),
        Err(e) => { crate::out!("  [FAIL] cache dir: {}", e); failures += 1; }
    }

    crate::out!("");
    if failures > 0 {
        Err(CompileError::new(format!("doctor: {} check(s) failed", failures)))
    } else {
        crate::out!("  all checks passed");
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

// ===================== 专业帮助系统 =====================
//
// 顶层帮助含 REFERENCES；每个子命令有独立帮助（NAME/SYNOPSIS/DESCRIPTION/
// OPTIONS/EXAMPLES/SEE ALSO），并通过「引用」交叉关联命令与设计文档。

struct CommandDoc {
    name: &'static str,
    summary: &'static str,
    usage: &'static str,
    description: &'static str,
    options: &'static [&'static str],
    examples: &'static [&'static str],
    see_also: &'static [&'static str],
}

const COMMAND_DOCS: &[CommandDoc] = &[
    CommandDoc {
        name: "gen",
        summary: "从 IDL 生成 Rust / C++ / anchor / 文档 / 测试脚手架",
        usage: "xso-idlc gen --input <file.xso> [--input ...] [options]",
        description:
            "解析并语义校验 IDL，构建桥接 IR，按中间布局规范生成多语言绑定。\n\
             多个 --input 之间并行构建；默认启用增量缓存（解析前查缓存，跳过未变更的解析与生成）。",
        options: &[
            "--input <path>      IDL 输入（可重复，跨输入并行）",
            "--anchor <path>     布局锚点头文件",
            "--cpp-out <path>    C++ 头文件",
            "--rust-out <path>   Rust 模块",
            "--doc-out <path>    Markdown ABI 文档",
            "--test-out <path>   布局对拍测试脚手架",
            "--emit-ir <path>    导出版本化 IR（JSON，含 ABI diff）",
            "--sbom <path>       导出 CycloneDX SBOM（SHA-256）",
            "--cache-dir <path>  增量缓存目录（默认 .xso-cache）",
            "--jobs <N>          并行构建线程上限（默认：全部核心）",
            "--no-cache          禁用增量缓存，强制全量重建",
        ],
        examples: &[
            "xso-idlc gen --input src/examples/fs.xso --rust-out build/fs_gen.rs",
            "xso-idlc gen --input a.xso --input b.xso --cpp-out out --jobs 4",
        ],
        see_also: &["check", "diff", "ir", "docs/XSO-IDLC-INDUSTRIALIZATION.md"],
    },
    CommandDoc {
        name: "check",
        summary: "校验已生成产物是否最新（CI 门禁，不写盘）",
        usage: "xso-idlc check --input <file.xso> [options]",
        description:
            "仅比对磁盘产物与本次生成结果，不一致或缺失即报错并退出非零。\n\
             用于 CI 门禁：保证提交的绑定代码与 IDL 始终保持同步。",
        options: &[
            "--input <path>      IDL 输入",
            "--anchor/--cpp-out/--rust-out/--doc-out/--test-out <path>",
            "--cache-dir <path>  增量缓存目录",
        ],
        examples: &["xso-idlc check --input src/examples/kv.xso"],
        see_also: &["gen", "docs/OPTIMIZATION-ROADMAP.md"],
    },
    CommandDoc {
        name: "ir",
        summary: "打印 ABI 指纹 / IR 摘要（--json / --bin 导出）",
        usage: "xso-idlc ir --input <file.xso> [--json <path>] [--bin <path>]",
        description:
            "构建桥接 IR 并打印模块名、版本、IR 版本、ABI 指纹（FNV-1a）、布局清单。\n\
             --json 导出版本化 IR（可跨版本 diff）；--bin 导出确定性二进制 IR（可离线反序列化做真实 diff）。",
        options: &[
            "--input <path>      IDL 输入",
            "--json <path>       导出 IR 为 JSON",
            "--bin <path>        导出 IR 为二进制",
        ],
        examples: &[
            "xso-idlc ir --input src/examples/net.xso",
            "xso-idlc ir --input net.xso --bin net.irbin",
        ],
        see_also: &["diff", "gen", "docs/XSO-IDLC-INDUSTRIALIZATION.md"],
    },
    CommandDoc {
        name: "diff",
        summary: "ABI 差异比对（含 [break] 破坏性判定）",
        usage: "xso-idlc diff --input <file.xso> --base <file.xso | ir.json | ir.bin>",
        description:
            "与基线比对布局，输出人类可读差异。破坏性变更以 [break] 前缀标记并返回非零退出码。\n\
             --base 可为 .xso（实时构建 IR）、.json（提取指纹）或 .bin（离线反序列化做真实 diff）。",
        options: &[
            "--input <path>      新 IDL 输入",
            "--base <path>       基线（.xso | .ir.json | .ir.bin）",
        ],
        examples: &[
            "xso-idlc diff --input new.xso --base old.xso",
            "xso-idlc diff --input new.xso --base old.irbin",
        ],
        see_also: &["ir", "check"],
    },
    CommandDoc {
        name: "migrate",
        summary: "就地升级旧版 IDL 语法（幂等）",
        usage: "xso-idlc migrate --input <file.xso>",
        description:
            "将旧式 IDL 语法改写为当前语法（如 result<(),E> → unit、@module NAME → (@NAME) 等）。\n\
             规则幂等：重复运行不产生变化。",
        options: &["--input <path>      IDL 输入（就地改写）"],
        examples: &["xso-idlc migrate --input legacy.xso"],
        see_also: &["gen", "docs/OPTIMIZATION-ROADMAP.md"],
    },
    CommandDoc {
        name: "sbom",
        summary: "导出 CycloneDX SBOM（SHA-256 产物清单）",
        usage: "xso-idlc sbom --input <file.xso> --out <file.json>",
        description:
            "生成 CycloneDX 1.4 SBOM，记录每个产物的 SHA-256 哈希，用于供应链审计与完整性追踪。",
        options: &[
            "--input <path>      IDL 输入",
            "--out <path>        SBOM 输出（也可用 --sbom）",
        ],
        examples: &["xso-idlc sbom --input fs.xso --out fs.sbom.json"],
        see_also: &["gen", "docs/XSO-IDLC-INDUSTRIALIZATION.md"],
    },
    CommandDoc {
        name: "doctor",
        summary: "自检：示例可解析、生成确定性、缓存可写",
        usage: "xso-idlc doctor",
        description:
            "运行内置自检：解析全部示例、验证生成确定性（同输入两次生成一致）、确认缓存目录可写。\n\
             用于安装 / 环境验证。",
        options: &["（无参数）"],
        examples: &["xso-idlc doctor"],
        see_also: &["gen", "check"],
    },
    CommandDoc {
        name: "interactive",
        summary: "菜单式交互模式（别名 ui）",
        usage: "xso-idlc interactive   |   xso-idlc ui   |   无参数",
        description: "进入菜单驱动模式，引导选择生成目标与输出，复用 gen 逻辑。",
        options: &["（无参数）"],
        examples: &["xso-idlc", "xso-idlc ui"],
        see_also: &["gen"],
    },
    CommandDoc {
        name: "completions",
        summary: "生成 shell 补全脚本（bash|zsh|fish|powershell）",
        usage: "xso-idlc completions <bash|zsh|fish|powershell>",
        description: "输出对应 shell 的补全脚本，补全子命令与通用选项。",
        options: &["<shell>   目标 shell"],
        examples: &["xso-idlc completions bash >> ~/.bashrc"],
        see_also: &["help"],
    },
];

fn find_doc(name: &str) -> Option<&'static CommandDoc> {
    COMMAND_DOCS.iter().find(|d| d.name == name)
}

pub fn print_help() {
    print_general_help();
}

pub fn print_general_help() {
    crate::out!("xso-idlc {} — XuanShu IDL Compiler", crate::GEN_VERSION);
    crate::out!("");
    crate::out!("NAME");
    crate::out!("    xso-idlc - XuanShu 跨语言 IDL 编译器（Rust / C++ 桥接）");
    crate::out!("");
    crate::out!("SYNOPSIS");
    crate::out!("    xso-idlc <command> [options]");
    crate::out!("    xso-idlc --input <file.xso> [options]   (旧式 flag 形式，向后兼容)");
    crate::out!("    xso-idlc                                (无参数进入交互模式)");
    crate::out!("");
    crate::out!("参数约定与校验 (PARAMETERS)");
    crate::out!("    两种调用形式：");
    crate::out!("      • 子命令形式：xso-idlc <command> [options]              （推荐）");
    crate::out!("      • 旧式 flag 形式：xso-idlc --input <file.xso> [options]  （向后兼容）");
    crate::out!("");
    crate::out!("    选项语法 (OPTION SYNTAX)");
    crate::out!("      • 长选项 --name <value> 与 --name=<value> 等价");
    crate::out!("      • 短写：-i = --input，-o = --out");
    crate::out!("      • --input 可重复出现，多个文件并行构建");
    crate::out!("      • 各子命令支持的选项见其 OPTIONS 段落");
    crate::out!("");
    crate::out!("    参数校验 (VALIDATION，失败均以非零退出码结束)");
    crate::out!("      • 缺 --input：报错（如 'gen: missing --input <file.xso>'）");
    crate::out!("      • 选项缺取值（如 --input 后无文件）：报错并退出，不 panic");
    crate::out!("      • 未知参数 / 未知子命令：报错并提示可用命令");
    crate::out!("      • 输入文件不存在或解析失败：逐条报告（recovery 模式不中断）");
    crate::out!("");
    crate::out!("COMMANDS");
    for d in COMMAND_DOCS {
        crate::out!("    {:<12} {}", d.name, d.summary);
    }
    crate::out!("");
    crate::out!("EXAMPLES (调用示例)");
    crate::out!("    # 生成 Rust / C++ / anchor 代码与文档到 ./gen 目录");
    crate::out!("    xso-idlc gen --input kv.xso --out ./gen");
    crate::out!("");
    crate::out!("    # 仅校验语法与语义，不生成任何文件");
    crate::out!("    xso-idlc check --input fs.xso");
    crate::out!("");
    crate::out!("    # 查看中间表示 (IR) 与 ABI 指纹");
    crate::out!("    xso-idlc ir --input net.xso");
    crate::out!("");
    crate::out!("    # 迁移旧版语法到当前版本（可原地覆盖）");
    crate::out!("    xso-idlc migrate --input old.xso --out new.xso");
    crate::out!("");
    crate::out!("    # 生成 SBOM（软件物料清单 JSON）");
    crate::out!("    xso-idlc sbom --input kv.xso --json sbom.json");
    crate::out!("");
    crate::out!("    # 无参数进入交互模式，菜单式引导生成");
    crate::out!("    xso-idlc");
    crate::out!("");
    crate::out!("GLOBAL OPTIONS");
    crate::out!("    -h, --help          显示帮助（可接 <command> 查看该命令详细说明）");
    crate::out!("    -V, --version       显示版本");
    crate::out!("");
    crate::out!("REFERENCES");
    crate::out!("    设计文档   ：docs/XSO-IDLC-INDUSTRIALIZATION.md");
    crate::out!("    优化路线   ：docs/OPTIMIZATION-ROADMAP.md");
    crate::out!("    生成 ABI 文档：<module>.md（由 gen --doc-out 产出）");
    crate::out!("");
    crate::out!("SEE ALSO");
    crate::out!("    xso-idlc help <command>   查看某子命令的详细说明、选项与示例");
}

fn print_command_help(name: &str) {
    match find_doc(name) {
        Some(d) => {
            crate::out!("NAME");
            crate::out!("    xso-idlc {} - {}", d.name, d.summary);
            crate::out!("");
            crate::out!("SYNOPSIS");
            crate::out!("    {}", d.usage);
            crate::out!("");
            crate::out!("DESCRIPTION");
            for line in d.description.lines() { crate::out!("    {}", line); }
            crate::out!("");
            crate::out!("OPTIONS");
            for o in d.options { crate::out!("    {}", o); }
            if !d.examples.is_empty() {
                crate::out!("");
                crate::out!("EXAMPLES");
                for e in d.examples { crate::out!("    {}", e); }
            }
            crate::out!("");
            crate::out!("SEE ALSO");
            for s in d.see_also { crate::out!("    {}", s); }
        }
        None => {
            crate::error!("unknown command: {}", name);
            print_general_help();
        }
    }
}

fn cmd_help(args: &[String]) -> Result<()> {
    if args.is_empty() { print_general_help(); }
    else { print_command_help(&args[0]); }
    Ok(())
}
