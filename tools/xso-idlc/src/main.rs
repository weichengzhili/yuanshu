mod ast;
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
  let mut cpp_out = None;
  let mut rust_out = None;
  let mut doc_out = None;
  let mut test_out = None;

  let mut i = 1;
  while i < args.len() {
    match args[i].as_str() {
      "--input"    => { input    = Some(PathBuf::from(&args[i+1])); i += 2; }
      "--cpp-out"  => { cpp_out  = Some(PathBuf::from(&args[i+1])); i += 2; }
      "--rust-out" => { rust_out = Some(PathBuf::from(&args[i+1])); i += 2; }
      "--doc-out"  => { doc_out  = Some(PathBuf::from(&args[i+1])); i += 2; }
      "--test-out" => { test_out = Some(PathBuf::from(&args[i+1])); i += 2; }
      "--help" | "-h" => {
        print_help();
        return Ok(());
      }
      "--version" | "-V" => {
        println!("xso-idlc 0.1.0");
        return Ok(());
      }
      other => {
        return Err(diag::CompileError::new(
          format!("unknown argument: {}", other)));
      }
    }
  }

  let input = input.ok_or_else(||
      diag::CompileError::new("missing --input <file.xso>"))?;

  let src = fs::read_to_string(&input)
      .map_err(|e| diag::CompileError::new(format!(
        "cannot read {}: {}", input.display(), e)))?;

  // 1. 词法 + 语法
  let module = parser::parse(&src)
      .map_err(|e| e.in_file(input.display().to_string()))?;

  eprintln!("[idlc] parsed module '{}' v{} ({} items)",
            module.name, module.version, module.items.len());

  // 2. 语义分析
  semantic::analyze(&module)
      .map_err(|e| e.in_file(input.display().to_string()))?;
  eprintln!("[idlc] semantic analysis OK");

  // 3. 代码生成
  if let Some(path) = cpp_out {
    let code = codegen::cpp::generate(&module);
    fs::write(&path, code)
        .map_err(|e| diag::CompileError::new(format!(
          "cannot write {}: {}", path.display(), e)))?;
    eprintln!("[idlc] generated C++ header: {}", path.display());
  }

  if let Some(path) = rust_out {
    let code = codegen::rust::generate(&module);
    fs::write(&path, code)
        .map_err(|e| diag::CompileError::new(format!(
          "cannot write {}: {}", path.display(), e)))?;
    eprintln!("[idlc] generated Rust module: {}", path.display());
  }

  if let Some(path) = doc_out {
    let doc = codegen::doc::generate(&module);
    fs::write(&path, doc)
        .map_err(|e| diag::CompileError::new(format!(
          "cannot write {}: {}", path.display(), e)))?;
    eprintln!("[idlc] generated doc: {}", path.display());
  }

  if let Some(path) = test_out {
    let test = codegen::test::generate(&module);
    fs::write(&path, test)
        .map_err(|e| diag::CompileError::new(format!(
          "cannot write {}: {}", path.display(), e)))?;
    eprintln!("[idlc] generated test skeleton: {}", path.display());
  }

  Ok(())
}

fn print_help() {
  println!("xso-idlc 0.1.0 - XuanShu IDL Compiler");
  println!();
  println!("USAGE:");
  println!("    xso-idlc --input <file.xso> [options]");
  println!();
  println!("OPTIONS:");
  println!("    --input    <path>  Input IDL file (required)");
  println!("    --cpp-out  <path>  Generate C++ header");
  println!("    --rust-out <path>  Generate Rust module");
  println!("    --doc-out  <path>  Generate Markdown doc");
  println!("    --test-out <path>  Generate test skeleton");
  println!("    --help, -h         Show this help");
  println!("    --version, -V      Show version");
}