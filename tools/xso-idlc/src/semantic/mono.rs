//! 泛型单态化（monomorphization）
//!
//! 设计：在语义阶段把用户泛型结构体 `Wrapper<T>` 按实际使用的类型实参
//! 展开为**具体结构体** `Wrapper__u32`，并把所有 `App{name,args}` 引用改写为
//! `Named(单态名)`。这样下游（布局计算 / anchor / Rust / C++ 代码生成）
//! 只看到普通具名结构体，**跨语言布局天然一致**，无需在两端各自做模板实例化。
//!
//! 支持嵌套泛型（如 `struct A<T> { b: B<T> }`）：采用有界不动点迭代展开。

use crate::ast::*;
use crate::diag::{CompileError, Result};
use std::collections::BTreeMap;

/// 不动点迭代轮次上限（防病态递归）
const MAX_ROUNDS: usize = 16;

pub fn monomorphize(module: &mut Module) -> Result<()> {
    // 泛型模板
    let generics: Vec<StructDef> = module.items.iter().filter_map(|it| match it {
        Item::Struct(s) if !s.type_params.is_empty() => Some(s.clone()),
        _ => None,
    }).collect();
    if generics.is_empty() { return Ok(()); }

    let mut instances: Vec<(String, String, Vec<TypeRef>)> = Vec::new();

    for _round in 0..MAX_ROUNDS {
        let start = instances.len();
        // 1) 遍历现有条目：改写 App -> Named(单态名)，同时登记新实例
        for it in module.items.iter_mut() {
            match it {
                Item::Struct(s) if s.type_params.is_empty() => {
                    for f in s.fields.iter_mut() {
                        walk_ty(&mut f.ty, &generics, &mut instances)?;
                    }
                }
                Item::Interface(i) => {
                    for m in i.methods.iter_mut() {
                        for p in m.params.iter_mut() {
                            walk_ty(&mut p.ty, &generics, &mut instances)?;
                        }
                        walk_ty(&mut m.ret, &generics, &mut instances)?;
                    }
                }
                Item::TypeAlias(t) => { walk_ty(&mut t.target, &generics, &mut instances)?; }
                _ => {}
            }
        }

        // 2) 为本轮新增实例生成具体结构体（其内部若还有泛型，下一轮继续展开）
        let newly = instances[start..].to_vec();
        if newly.is_empty() { break; }
        for (mangled, tname, args) in newly {
            let tpl = generics.iter().find(|g| g.name == tname)
                .ok_or_else(|| CompileError::new(format!("unknown generic '{}'", tname)))?;
            let map = bind(&tpl.type_params, &args)?;
            let fields = tpl.fields.iter().map(|f| Field {
                name: f.name.clone(),
                ty: subst(&f.ty, &map),
                annotations: f.annotations.clone(),
            }).collect();
            module.items.push(Item::Struct(StructDef {
                name: mangled,
                opaque: tpl.opaque,
                type_params: Vec::new(),
                fields,
            }));
        }
    }

    // 3) 移除泛型模板（只保留单态后的具体结构体）
    module.items.retain(|it| match it {
        Item::Struct(s) => s.type_params.is_empty(),
        _ => true,
    });
    Ok(())
}

/// 遍历类型：把 App 改写为 Named(单态名) 并登记实例
fn walk_ty(
    ty: &mut TypeRef,
    generics: &[StructDef],
    instances: &mut Vec<(String, String, Vec<TypeRef>)>,
) -> Result<()> {
    match ty {
        TypeRef::App { name, args } => {
            // 先递归处理实参（支持嵌套泛型）
            for a in args.iter_mut() { walk_ty(a, generics, instances)?; }
            let tpl = generics.iter().find(|g| &g.name == name)
                .ok_or_else(|| CompileError::new(format!("unknown generic type '{}'", name)))?;
            // 实参元数校验
            let _ = bind(&tpl.type_params, args)?;
            let mangled = mangle(name, args);
            if !instances.iter().any(|(m, _, _)| m == &mangled) {
                instances.push((mangled.clone(), name.clone(), args.clone()));
            }
            *ty = TypeRef::Named(mangled);
        }
        TypeRef::Vec(i) | TypeRef::Option(i) | TypeRef::Set(i)
        | TypeRef::Owned(i) | TypeRef::Borrowed(i) => walk_ty(i, generics, instances)?,
        TypeRef::Map(k, v) | TypeRef::Result(k, v) => {
            walk_ty(k, generics, instances)?;
            walk_ty(v, generics, instances)?;
        }
        _ => {}
    }
    Ok(())
}

/// 形参 -> 实参 绑定（含元数校验）
fn bind(params: &[String], args: &[TypeRef]) -> Result<BTreeMap<String, TypeRef>> {
    if params.len() != args.len() {
        return Err(CompileError::new(format!(
            "generic arity mismatch: expected {} type argument(s), got {}",
            params.len(), args.len()
        )));
    }
    Ok(params.iter().cloned().zip(args.iter().cloned()).collect())
}

/// 类型替换：把具名类型形参替换为实参
fn subst(ty: &TypeRef, map: &BTreeMap<String, TypeRef>) -> TypeRef {
    match ty {
        TypeRef::Named(n) => map.get(n).cloned().unwrap_or_else(|| ty.clone()),
        TypeRef::App { name, args } => TypeRef::App {
            name: name.clone(),
            args: args.iter().map(|a| subst(a, map)).collect(),
        },
        TypeRef::Vec(i) => TypeRef::Vec(Box::new(subst(i, map))),
        TypeRef::Option(i) => TypeRef::Option(Box::new(subst(i, map))),
        TypeRef::Set(i) => TypeRef::Set(Box::new(subst(i, map))),
        TypeRef::Owned(i) => TypeRef::Owned(Box::new(subst(i, map))),
        TypeRef::Borrowed(i) => TypeRef::Borrowed(Box::new(subst(i, map))),
        TypeRef::Map(k, v) => TypeRef::Map(Box::new(subst(k, map)), Box::new(subst(v, map))),
        TypeRef::Result(o, e) => TypeRef::Result(Box::new(subst(o, map)), Box::new(subst(e, map))),
        other => other.clone(),
    }
}

/// 单态名：`Wrapper__u32`、`Pair__u32_Vec_u8_`
pub fn mangle(name: &str, args: &[TypeRef]) -> String {
    let mut s = format!("{}__", name);
    for (i, a) in args.iter().enumerate() {
        if i > 0 { s.push('_'); }
        s.push_str(&ty_key(a));
    }
    s
}

/// 类型的稳定键（用于单态名与 IR 描述）
pub fn ty_key(t: &TypeRef) -> String {
    match t {
        TypeRef::Primitive(p) => p.clone(),
        TypeRef::Named(n) => n.clone(),
        TypeRef::BorrowedStr => "BorrowedStr".into(),
        TypeRef::OwnedString => "OwnedString".into(),
        TypeRef::App { name, args } => mangle(name, args),
        TypeRef::Vec(i) => format!("Vec_{}_", ty_key(i)),
        TypeRef::Set(i) => format!("Set_{}_", ty_key(i)),
        TypeRef::Option(i) => format!("Option_{}_", ty_key(i)),
        TypeRef::Owned(i) => format!("Owned_{}_", ty_key(i)),
        TypeRef::Borrowed(i) => format!("Borrowed_{}_", ty_key(i)),
        TypeRef::Map(k, v) => format!("Map_{}_{}_", ty_key(k), ty_key(v)),
        TypeRef::Result(o, e) => format!("Result_{}_{}_", ty_key(o), ty_key(e)),
    }
}
