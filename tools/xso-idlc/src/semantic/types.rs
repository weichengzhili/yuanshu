use crate::ast::*;
use crate::diag::{CompileError, Result};

const PRIMITIVES: &[&str] = &[
    "u8", "u16", "u32", "u64",
    "i8", "i16", "i32", "i64",
    "bool", "unit",
];

pub fn check_all(module: &Module) -> Result<()> {
    // 收集所有定义的名称
    let defined: Vec<&str> = module.items.iter().map(|it| match it {
        Item::Enum(e) => e.name.as_str(),
        Item::Error(e) => e.name.as_str(),
        Item::Struct(s) => s.name.as_str(),
        Item::Interface(i) => i.name.as_str(),
        Item::TypeAlias(t) => t.name.as_str(),
    }).collect();

    for item in &module.items {
        match item {
            Item::Struct(s) => {
                for f in &s.fields {
                    check_type(&f.ty, &defined, &s.name, &f.name)?;
                }
            }
            Item::Interface(i) => {
                for m in &i.methods {
                    for p in &m.params {
                        check_type(&p.ty, &defined, &i.name, &m.name)?;
                    }
                    check_type(&m.ret, &defined, &i.name, &m.name)?;
                }
            }
            Item::TypeAlias(t) => {
                check_type(&t.target, &defined, &t.name, "alias")?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn check_type(ty: &TypeRef, defined: &[&str], ctx_type: &str, ctx_member: &str) -> Result<()> {
    match ty {
        TypeRef::Primitive(p) => {
            if !PRIMITIVES.contains(&p.as_str()) {
                return Err(CompileError::new(format!(
                    "unknown primitive '{}' in {}.{}", p, ctx_type, ctx_member
                )));
            }
        }
        TypeRef::BorrowedStr | TypeRef::OwnedString => {}
        TypeRef::Named(n) => {
            if !defined.contains(&n.as_str()) {
                return Err(CompileError::new(format!(
                    "unknown type '{}' referenced in {}.{}", n, ctx_type, ctx_member
                )));
            }
        }
        TypeRef::Vec(inner) | TypeRef::Option(inner) => {
            check_type(inner, defined, ctx_type, ctx_member)?;
        }
        TypeRef::Result(ok, err) => {
            check_type(ok, defined, ctx_type, ctx_member)?;
            check_type(err, defined, ctx_type, ctx_member)?;
        }
    }
    Ok(())
}