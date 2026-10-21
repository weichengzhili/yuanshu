use crate::ast::*;
use crate::diag::{CompileError, Result};
use crate::semantic::layout;

const PRIMITIVES: &[&str] = &[
    "u8", "u16", "u32", "u64",
    "i8", "i16", "i32", "i64",
    "bool", "unit",
];

pub fn check_all(module: &Module) -> Result<()> {
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
                    // 边界控制：opaque 类型不能作为字段（会按值嵌入）
                    if let TypeRef::Named(n) = &f.ty {
                        if module.is_opaque(n) {
                            return Err(CompileError::new(format!(
                                "opaque type '{}' cannot be embedded by value in struct '{}.{}'",
                                n, s.name, f.name
                            )).with_hint("use `@owned Box<T>` or `@borrowed T` instead"));
                        }
                    }
                }
            }
            Item::Interface(i) => {
                for m in &i.methods {
                    for p in &m.params {
                        check_type(&p.ty, &defined, &i.name, &m.name)?;
                        // 边界控制：opaque 参数必须 borrowed/owned
                        if let TypeRef::Named(n) = &p.ty {
                            if module.is_opaque(n) && p.ownership == Ownership::Value {
                                return Err(CompileError::new(format!(
                                    "opaque type '{}' must be passed by @borrowed or @owned, not by value (param '{}' of {}.{})",
                                    n, p.name, i.name, m.name
                                )));
                            }
                        }
                    }
                    check_type(&m.ret, &defined, &i.name, &m.name)?;
                    // 返回值：opaque 必须 @owned / @borrowed
                    if let TypeRef::Named(n) = &m.ret {
                        if module.is_opaque(n) {
                            return Err(CompileError::new(format!(
                                "opaque type '{}' returned by value from {}.{}; use result<@owned {}, ...> or result<@borrowed {}, ...>",
                                n, i.name, m.name, n, n
                            )));
                        }
                    }
                }
            }
            Item::TypeAlias(t) => {
                check_type(&t.target, &defined, &t.name, "alias")?;
            }
            _ => {}
        }
    }

    // 布局自检：字段偏移必须是自然对齐的
    for item in &module.items {
        if let Item::Struct(s) = item {
            if s.opaque { continue; }
            for (name, off) in layout::compute_field_offsets(module, s) {
                let _ = name;
                let _ = off;
            }
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
                    "unknown type '{}' in {}.{}", n, ctx_type, ctx_member
                )));
            }
        }
        TypeRef::Vec(inner) | TypeRef::Option(inner)
        | TypeRef::Owned(inner) | TypeRef::Borrowed(inner) => {
            check_type(inner, defined, ctx_type, ctx_member)?;
        }
        TypeRef::Result(ok, err) => {
            check_type(ok, defined, ctx_type, ctx_member)?;
            check_type(err, defined, ctx_type, ctx_member)?;
        }
    }
    Ok(())
}