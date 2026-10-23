//! 交互式 REPL：无参数启动或 `interactive`/`ui` 时进入。
//!
//! 采用英文命令格式（与 CLI 子命令一致）：在 `xso-idlc>` 提示符下键入
//! `gen --input foo.xso --rust-out out.rs` 等，内部直接复用 `cli::dispatch`，
//! 保证交互与非交互行为完全一致。

use crate::cli;
use std::io::{self, Write};
use crate::diag::Result;

pub fn run_interactive() -> Result<()> {
    println!("xso-idlc interactive shell");
    println!("  Type a command (gen / check / ir / diff / migrate / sbom / doctor),");
    println!("  e.g.  gen --input src/examples/fs.xso --rust-out out.rs");
    println!("  `help` for command list, `quit`/`exit`/`Ctrl-D` to leave.");
    loop {
        print!("xso-idlc> ");
        io::stdout().flush().ok();
        let mut line = String::new();
        let n = io::stdin().read_line(&mut line)
            .map_err(|e| crate::diag::CompileError::new(format!("read stdin failed: {}", e)))?;
        if n == 0 { break; } // EOF
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() { continue; }
        let lower = line.to_ascii_lowercase();
        match lower.as_str() {
            "quit" | "exit" | "q" => { println!("bye."); break; }
            "interactive" | "ui" => { println!("already in interactive mode."); continue; }
            "?" => { cli::print_general_help(); continue; }
            _ => {}
        }
        let mut tokens: Vec<String> = vec!["xso-idlc".to_string()];
        tokens.extend(line.split_whitespace().map(|s| s.to_string()));
        if let Err(e) = cli::dispatch(&tokens) {
            eprintln!("error: {}", e);
        }
    }
    Ok(())
}
