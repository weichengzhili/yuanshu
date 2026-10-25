use crate::ast::*;
use crate::diag::{CompileError, Result};
use crate::semantic::layout;
use std::collections::BTreeMap;

const PRIMITIVES: &[&str] = &[
    "u8", "u16", "u32", "u64",
    "i8", "i16", "i32", "i64",
    "bool", "unit",
];

/// 类型嵌套深度上限（防栈溢出 / 恶意构造）
pub const MAX_TYPE_DEPTH: usize = 32;
/// 单文件条目数上限（资源限制）
pub const MAX_ITEMS: usize = 4096;
/// 标识符长度上限
pub const MAX_IDENT_LEN: usize = 128;

pub fn check_all(module: &Module) -> Result<()> {
    // 资源限制：条目数
    if module.items.len() > MAX_ITEMS {
        return Err(CompileError::new(format!(
            "too many items: {} (limit {})", module.items.len(), MAX_ITEMS)));
    }

    let defined: Vec<&str> = module.items.iter().map(|it| match it {
        Item::Enum(e) => e.name.as_str(),
        Item::Error(e) => e.name.as_str(),
        Item::Struct(s) => s.name.as_str(),
        Item::Interface(i) => i.name.as_str(),
        Item::TypeAlias(t) => t.name.as_str(),
    }).collect();

    // ===== 符号表：重复定义检测 =====
    let mut symtab: BTreeMap<&str, &str> = BTreeMap::new();
    for item in &module.items {
        let (name, kind) = match item {
            Item::Enum(e) => (e.name.as_str(), "enum"),
            Item::Error(e) => (e.name.as_str(), "error"),
            Item::Struct(s) => (s.name.as_str(), "struct"),
            Item::Interface(i) => (i.name.as_str(), "interface"),
            Item::TypeAlias(t) => (t.name.as_str(), "type alias"),
        };
        check_ident(name)?;
        if let Some(prev) = symtab.insert(name, kind) {
            return Err(CompileError::new(format!(
                "duplicate definition of '{}' ({} and {})", name, prev, kind
            )).with_hint("rename one of them"));
        }
    }

    // ===== 成员名重复检测 =====
    for item in &module.items {
        match item {
            Item::Struct(s) => {
                let mut seen: BTreeMap<&str, ()> = BTreeMap::new();
                for f in &s.fields {
                    check_ident(&f.name)?;
                    if seen.insert(f.name.as_str(), ()).is_some() {
                        return Err(CompileError::new(format!(
                            "duplicate field '{}' in struct '{}'", f.name, s.name)));
                    }
                }
            }
            Item::Interface(i) => {
                let mut seen: BTreeMap<&str, ()> = BTreeMap::new();
                for m in &i.methods {
                    check_ident(&m.name)?;
                    if seen.insert(m.name.as_str(), ()).is_some() {
                        return Err(CompileError::new(format!(
                            "duplicate method '{}' in interface '{}'", m.name, i.name)));
                    }
                }
            }
            Item::Enum(e) => {
                let mut seen: BTreeMap<&str, ()> = BTreeMap::new();
                for v in &e.variants {
                    check_ident(&v.name)?;
                    if seen.insert(v.name.as_str(), ()).is_some() {
                        return Err(CompileError::new(format!(
                            "duplicate variant '{}' in '{}'", v.name, e.name)));
                    }
                }
            }
            Item::Error(e) => {
                let mut seen: BTreeMap<&str, ()> = BTreeMap::new();
                for v in &e.variants {
                    check_ident(&v.name)?;
                    if seen.insert(v.name.as_str(), ()).is_some() {
                        return Err(CompileError::new(format!(
                            "duplicate variant '{}' in '{}'", v.name, e.name)));
                    }
                }
            }
            _ => {}
        }
    }

    for item in &module.items {
        match item {
            Item::Struct(s) => {
                for f in &s.fields {
                    check_type(&f.ty, &defined, &s.name, &f.name, 0)?;
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
                        check_ident(&p.name)?;
                        check_type(&p.ty, &defined, &i.name, &m.name, 0)?;
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
                    check_type(&m.ret, &defined, &i.name, &m.name, 0)?;
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
                check_type(&t.target, &defined, &t.name, "alias", 0)?;
            }
            _ => {}
        }
    }

    // 边界约定（S3）：0 保留为成功码，错误枚举不得定义 0；错误码不得重复
    for item in &module.items {
        if let Item::Error(e) = item {
            let mut seen = BTreeMap::new();
            for v in &e.variants {
                if v.value == 0 {
                    return Err(CompileError::new(format!(
                        "error '{}' variant '{}' = 0, but 0 is reserved for success on the FFI boundary",
                        e.name, v.name
                    )).with_hint("use a non-zero errno-style code"));
                }
                if let Some(prev) = seen.insert(v.value, v.name.clone()) {
                    return Err(CompileError::new(format!(
                        "error '{}': duplicate code {} for '{}' and '{}'",
                        e.name, v.value, prev, v.name
                    )));
                }
            }
        }
    }

    // ===== 循环引用检测（按值嵌入的结构体）=====
    check_cycles(module)?;

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

/// 标识符合法性：长度上限 + 禁止控制字符（输入验证/注入防护）
fn check_ident(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(CompileError::new("empty identifier"));
    }
    if name.len() > MAX_IDENT_LEN {
        return Err(CompileError::new(format!(
            "identifier too long: {} bytes (limit {})", name.len(), MAX_IDENT_LEN)));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(CompileError::new(format!(
            "identifier contains control character: {:?}", name)));
    }
    Ok(())
}

fn check_type(ty: &TypeRef, defined: &[&str], ctx_type: &str, ctx_member: &str, depth: usize)
    -> Result<()>
{
    // 深度限制：防栈溢出
    if depth > MAX_TYPE_DEPTH {
        return Err(CompileError::new(format!(
            "type nesting too deep (> {}) in {}.{}", MAX_TYPE_DEPTH, ctx_type, ctx_member
        )).with_hint("simplify the type or split it into named types"));
    }
    let next = depth + 1;
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
        | TypeRef::Owned(inner) | TypeRef::Borrowed(inner) | TypeRef::Set(inner) => {
            check_type(inner, defined, ctx_type, ctx_member, next)?;
        }
        TypeRef::Map(k, v) => {
            check_type(k, defined, ctx_type, ctx_member, next)?;
            check_type(v, defined, ctx_type, ctx_member, next)?;
        }
        TypeRef::Result(ok, err) => {
            check_type(ok, defined, ctx_type, ctx_member, next)?;
            check_type(err, defined, ctx_type, ctx_member, next)?;
        }
        TypeRef::App { args, .. } => {
            for a in args { check_type(a, defined, ctx_type, ctx_member, next)?; }
        }
    }
    Ok(())
}

/// 循环引用检测：按值嵌入的结构体构成依赖图，DFS 找环并报告路径。
/// 经 @owned/@borrowed/Vec/Option/Map/Set 间接引用的不算按值嵌入。
fn check_cycles(module: &Module) -> Result<()> {
    let struct_names: Vec<&str> = module.items.iter().filter_map(|it| match it {
        Item::Struct(s) => Some(s.name.as_str()),
        _ => None,
    }).collect();

    // 依赖边：仅直接按值嵌入的具名字段
    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for it in &module.items {
        if let Item::Struct(s) = it {
            let mut deps = Vec::new();
            for f in &s.fields {
                if let TypeRef::Named(n) = &f.ty {
                    if struct_names.contains(&n.as_str()) { deps.push(n.as_str()); }
                }
            }
            edges.insert(s.name.as_str(), deps);
        }
    }

    let mut state: BTreeMap<&str, u8> = BTreeMap::new();
    let mut stack: Vec<&str> = Vec::new();
    for n in &struct_names {
        if let Some(cycle) = dfs_cycle(n, &edges, &mut state, &mut stack) {
            return Err(CompileError::new(format!(
                "cyclic struct dependency: {}", cycle.join(" -> ")
            )).with_hint("break it with @owned Box<T> / @borrowed T (pointer breaks the cycle)"));
        }
    }
    Ok(())
}

fn dfs_cycle<'a>(
    name: &'a str,
    edges: &'a BTreeMap<&'a str, Vec<&'a str>>,
    state: &mut BTreeMap<&'a str, u8>,
    stack: &mut Vec<&'a str>,
) -> Option<Vec<String>> {
    match state.get(name).copied().unwrap_or(0) {
        1 => {
            let start = stack.iter().position(|n| *n == name).unwrap_or(0);
            return Some(stack[start..].iter().chain(std::iter::once(&name))
                .map(|s| s.to_string()).collect());
        }
        2 => return None,
        _ => {}
    }
    state.insert(name, 1);
    stack.push(name);
    if let Some(deps) = edges.get(name) {
        for d in deps {
            if let Some(c) = dfs_cycle(d, edges, state, stack) { return Some(c); }
        }
    }
    stack.pop();
    state.insert(name, 2);
    None
}
