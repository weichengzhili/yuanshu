use crate::ast::*;
use crate::diag::{CompileError, Result};

pub fn check_all(module: &Module) -> Result<()> {
    for item in &module.items {
        match item {
            Item::Struct(s) => check_indices(&s.name, s.fields.iter().map(|f| {
                f.annotations.iter().find(|a| a.name == "field")
                    .and_then(|a| a.int_arg(0))
            }))?,
            Item::Interface(i) => check_indices(&i.name, i.methods.iter().map(|m| {
                m.annotations.iter().find(|a| a.name == "method")
                    .and_then(|a| a.int_arg(0))
            }))?,
            _ => {}
        }
    }
    Ok(())
}

fn check_indices<'a>(name: &str, it: impl Iterator<Item = Option<i64>>) -> Result<()> {
    let mut seen = Vec::new();
    for idx in it.flatten() {
        if seen.contains(&idx) {
            return Err(CompileError::new(format!(
                "duplicate index {} in '{}'", idx, name
            )));
        }
        seen.push(idx);
    }
    Ok(())
}