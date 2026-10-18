use crate::ast::*;
use crate::diag::{CompileError, Result};

pub fn check_all(module: &Module) -> Result<()> {
    for item in &module.items {
        match item {
            Item::Struct(s) => check_field_indices(&s.name, &s.fields)?,
            Item::Interface(i) => check_method_indices(&i.name, &i.methods)?,
            _ => {}
        }
    }
    Ok(())
}

fn check_field_indices(struct_name: &str, fields: &[Field]) -> Result<()> {
    let mut seen = Vec::new();
    for f in fields {
        if let Some(field_ann) = f.annotations.iter().find(|a| a.name == "field") {
            if let Some(idx) = field_ann.int_arg(0) {
                if seen.contains(&idx) {
                    return Err(CompileError::new(format!(
                        "duplicate @field({}) in struct '{}'", idx, struct_name
                    )));
                }
                seen.push(idx);
            }
        }
    }
    Ok(())
}

fn check_method_indices(iface_name: &str, methods: &[Method]) -> Result<()> {
    let mut seen = Vec::new();
    for m in methods {
        if let Some(m_ann) = m.annotations.iter().find(|a| a.name == "method") {
            if let Some(idx) = m_ann.int_arg(0) {
                if seen.contains(&idx) {
                    return Err(CompileError::new(format!(
                        "duplicate @method({}) in interface '{}'", idx, iface_name
                    )));
                }
                seen.push(idx);
            }
        }
    }
    Ok(())
}