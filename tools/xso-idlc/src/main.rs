mod ast;
mod bridge_ir;
mod codegen;
mod diag;
mod lexer;
mod parser;
mod semantic;

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", e);
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> diag::Result<()> {
    let mut input = None;
    let mut anchor_out = None;
    let mut cpp_out = None;
    let mut rust_out = None;
    let mut doc_out = None;
    let mut test_out = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--input"    => { input      = Some(PathBuf::from(&args[i+1])); i += 2; }
            "--anchor"   => { anchor_out = Some(PathBuf::from(&args[i+1])); i += 2; }
            "--cpp-out"  => { cpp_out    = Some(PathBuf::from(&args[i+1])); i += 2; }
            "--rust-out" => { rust_out   = Some(PathBuf::from(&args[i+1])); i += 2; }
            "--doc-out"  => { doc_out    = Some(PathBuf::from(&args[i+1])); i += 2; }
            "--test-out" => { test_out   = Some(PathBuf::from(&args[i+1])); i += 2; }
            "--help" | "-h" => { print_help(); return Ok(()); }
            "--version" | "-V" => { println!("xso-idlc 0.1.0"); return Ok(()); }
            other => return Err(diag::CompileError::new(format!("unknown argument: {}", other))),
        }
    }

    let input = input.ok_or_else(|| diag::CompileError::new("missing --input <file.xso>"))?;
    let src = fs::read_to_string(&input)
        .map_err(|e| diag::CompileError::new(format!("cannot read {}: {}", input.display(), e)))?;

    let module = parser::parse(&src)
        .map_err(|e| e.in_file(input.display().to_string()))?;
    eprintln!("[idlc] parsed '{}' v{} ({} items)",
              module.name, module.version, module.items.len());

    semantic::analyze(&module)
        .map_err(|e| e.in_file(input.display().to_string()))?;
    eprintln!("[idlc] semantic OK");

    // 构建 Bridge IR（布局唯一真相）
    let ir = bridge_ir::BridgeIr::build(&module);
    eprintln!("[idlc] bridge IR: {} layouts, {} opaque",
              ir.type_layouts.len(), ir.opaque_types.len());

    // 所有 codegen 统一消费 &ir，避免两端各自算布局
    if let Some(path) = anchor_out {
        let code = codegen::anchor::generate(&ir);
        write_out(&path, &code)?;
    }
    if let Some(path) = cpp_out {
        let code = codegen::cpp::generate(&ir);
        write_out(&path, &code)?;
    }
    if let Some(path) = rust_out {
        let code = codegen::rust::generate(&ir);
        write_out(&path, &code)?;
    }
    if let Some(path) = doc_out {
        let code = codegen::doc::generate(&ir);
        write_out(&path, &code)?;
    }
    if let Some(path) = test_out {
        let code = codegen::test::generate(&ir);
        write_out(&path, &code)?;
    }

    Ok(())
}

fn write_out(path: &PathBuf, code: &str) -> diag::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).ok();
        }
    }
    fs::write(path, code)
        .map_err(|e| diag::CompileError::new(format!("cannot write {}: {}", path.display(), e)))?;
    eprintln!("[idlc] wrote {}", path.display());
    Ok(())
}

fn print_help() {
    println!("xso-idlc 0.1.0 — XuanShu IDL Compiler");
    println!();
    println!("USAGE:");
    println!("    xso-idlc --input <file.xso> [options]");
    println!();
    println!("OPTIONS:");
    println!("    --input    <path>   Input IDL (required)");
    println!("    --anchor   <path>   Layout anchor header (for both C++ and Rust)");
    println!("    --cpp-out  <path>   C++ header");
    println!("    --rust-out <path>   Rust module");
    println!("    --doc-out  <path>   Markdown doc");
    println!("    --test-out <path>   Layout test scaffold");
    println!("    --help, -h          This help");
    println!("    --version, -V       Version");
}
