//! 生成代码语法词汇表：Rust / C++ 的关键字、属性、断言与声明模板集中定义。
//!
//! 各后端（rust / cpp / anchor）只调用本模块提供的构造器，
//! **禁止**在生成逻辑里直接写死源码文本。这样改名、换惯用法、
//! 适配不同语言标准只需改这里一处。

/// Rust 侧语法
pub mod rs {
    pub const PUB: &str = "pub";
    pub const CONST: &str = "const";
    pub const STRUCT: &str = "struct";
    pub const ENUM: &str = "enum";
    pub const IMPL: &str = "impl";
    pub const FN: &str = "fn";
    pub const UNSAFE: &str = "unsafe";
    pub const EXTERN_C: &str = "extern \"C\"";
    pub const TYPE: &str = "type";
    pub const USE: &str = "use";
    pub const MOD: &str = "mod";
    pub const CORE: &str = "core";
    pub const DOC: &str = "///";
    /// 标量与枚举的默认 derive 列表
    pub const DERIVE_POD: [&str; 5] = ["Clone", "Copy", "PartialEq", "Eq", "Debug"];

    pub fn repr(what: &str) -> String { format!("#[repr({})]", what) }
    pub fn repr_c() -> String { repr("C") }
    pub fn derive(items: &[&str]) -> String { format!("#[derive({})]", items.join(", ")) }
    pub fn allow(items: &[&str]) -> String { format!("#![allow({})]", items.join(", ")) }
    pub fn doc(text: &str) -> String { format!("{} {}", DOC, text) }
    pub fn mod_open(name: &str) -> String { format!("{} {} {} {{", PUB, MOD, name) }
    pub fn use_stmt(path: &str) -> String { format!("{} {};", USE, path) }
    pub fn struct_open(name: &str) -> String { format!("{} {} {} {{", PUB, STRUCT, name) }
    pub fn enum_open(name: &str) -> String { format!("{} {} {} {{", PUB, ENUM, name) }
    pub fn impl_open(name: &str) -> String { format!("{} {} {{", IMPL, name) }
    pub fn field(name: &str, ty: &str) -> String { format!("{} {}: {},", PUB, name, ty) }
    pub fn variant(name: &str, val: i64) -> String { format!("{} = {},", name, val) }
    pub fn type_alias(name: &str, target: &str) -> String {
        format!("{} {} {} = {};", PUB, TYPE, name, target)
    }
    pub fn const_decl(is_pub: bool, name: &str, ty: &str, val: &str) -> String {
        let vis = if is_pub { format!("{} ", PUB) } else { String::new() };
        format!("{}{} {}: {} = {};", vis, CONST, name, ty, val)
    }
    pub fn assert_eq(lhs: &str, rhs: &str) -> String {
        format!("const _: () = assert!({} == {});", lhs, rhs)
    }
    pub fn size_of(ty: &str) -> String { format!("{}::mem::size_of::<{}>()", CORE, ty) }
    pub fn align_of(ty: &str) -> String { format!("{}::mem::align_of::<{}>()", CORE, ty) }
    pub fn offset_of(st: &str, field: &str) -> String {
        format!("{}::mem::offset_of!({}, {})", CORE, st, field)
    }
    /// VTable 中的函数指针字段
    pub fn fn_ptr(name: &str, params: &str, ret: &str) -> String {
        format!("{} {}: {} {} {}({}) -> {},", PUB, name, UNSAFE, EXTERN_C, FN, params, ret)
    }
    /// trait 中的方法声明
    pub fn trait_fn(name: &str, params: &str, ret: &str) -> String {
        format!("{} {}({}) -> {};", FN, name, params, ret)
    }
    pub fn trait_open(name: &str) -> String { format!("{} trait {} {{", PUB, name) }
    pub fn fn_open(name: &str, params: &str, ret: &str) -> String {
        format!("{} {}({}) -> {} {{", FN, name, params, ret)
    }
}

/// C++ 侧语法
pub mod cpp {
    pub const PRAGMA_ONCE: &str = "#pragma once";
    pub const INCLUDE: &str = "#include";
    pub const NAMESPACE: &str = "namespace";
    pub const STRUCT: &str = "struct";
    pub const ENUM_CLASS: &str = "enum class";
    pub const TEMPLATE: &str = "template";
    pub const STATIC_ASSERT: &str = "static_assert";
    pub const INLINE: &str = "inline";
    pub const STATIC_CONSTEXPR: &str = "static constexpr";
    pub const NORETURN: &str = "[[noreturn]]";
    pub const NOEXCEPT: &str = "noexcept";
    pub const IS_TRIVIALLY_COPYABLE: &str = "std::is_trivially_copyable";
    pub const DEFINE: &str = "#define";
    pub const CLASS: &str = "class";
    pub const PUBLIC: &str = "public:";

    pub fn include(path: &str, quoted: bool) -> String {
        if quoted { format!("{} \"{}\"", INCLUDE, path) } else { format!("{} <{}>", INCLUDE, path) }
    }
    pub fn namespace_open(name: &str) -> String { format!("{} {} {{", NAMESPACE, name) }
    pub fn struct_open(name: &str) -> String { format!("{} {} {{", STRUCT, name) }
    pub fn struct_close() -> String { "};".to_string() }
    pub fn enum_class_open(name: &str, underlying: &str) -> String {
        format!("{} {} : {} {{", ENUM_CLASS, name, underlying)
    }
    pub fn enum_class_close() -> String { "};".to_string() }
    pub fn template(params: &str) -> String { format!("{} <{}>", TEMPLATE, params) }
    pub fn member(ty: &str, name: &str) -> String { format!("{} {};", ty, name) }
    pub fn enumerator(name: &str, val: i64) -> String { format!("{} = {},", name, val) }
    pub fn static_assert(cond: &str, msg: &str) -> String {
        format!("{}({}, \"{}\");", STATIC_ASSERT, cond, msg)
    }
    /// 平凡拷贝构造（按寄存器传参的前提）
    pub fn copy_ctor_default(name: &str) -> String {
        format!("{}(const {}&) {} = default;", name, name, NOEXCEPT)
    }
    pub fn dtor_default(name: &str) -> String { format!("~{}() {} = default;", name, NOEXCEPT) }
    pub fn trivially_copyable(ty: &str) -> String {
        format!("{}<{}>::value", IS_TRIVIALLY_COPYABLE, ty)
    }
    pub fn constexpr_var(ty: &str, name: &str, val: &str) -> String {
        format!("{} {} {} = {};", STATIC_CONSTEXPR, ty, name, val)
    }
    pub fn fn_inline(ret: &str, name: &str, params: &str) -> String {
        format!("{} {} {}({}) {{", INLINE, ret, name, params)
    }
    pub fn fn_noreturn(ret: &str, name: &str, params: &str) -> String {
        format!("{} {} {}({}) {{", NORETURN, ret, name, params)
    }
    pub fn define(name: &str, val: &str) -> String { format!("{} {} {}", DEFINE, name, val) }
    pub fn class_open(name: &str) -> String { format!("{} {} {{", CLASS, name) }

    // ---- 语句与表达式构造器 ----
    pub fn struct_one_line(name: &str, body: &str) -> String {
        format!("{} {} {{ {} }};", STRUCT, name, body)
    }
    pub fn using_alias(name: &str, target: &str) -> String {
        format!("using {} = {};", name, target)
    }
    pub fn forward_decl(kind: &str, name: &str) -> String { format!("{} {};", kind, name) }
    /// vtable 中的函数指针字段：`ret (*name)(params);`
    pub fn fn_ptr_field(ret: &str, name: &str, params: &str) -> String {
        format!("{} (*{})({});", ret, name, params)
    }
    /// 成员方法定义头：`ret name(params) {`
    pub fn method_open(ret: &str, name: &str, params: &str) -> String {
        format!("{} {}({}) {{", ret, name, params)
    }
    pub fn switch_open(expr: &str) -> String { format!("switch ({}) {{", expr) }
    pub fn case(pat: &str, body: &str) -> String { format!("case {}: {};", pat, body) }
    pub fn default(body: &str) -> String { format!("default: {};", body) }
    pub fn var_init(ty: &str, name: &str, init: &str) -> String {
        format!("{} {} = {};", ty, name, init)
    }
    /// 值初始化：`T x{};`
    pub fn var_brace_init(ty: &str, name: &str) -> String { format!("{} {}{{}};", ty, name) }
    pub fn return_stmt(expr: &str) -> String { format!("return {};", expr) }
    pub fn if_stmt(cond: &str, body: &str) -> String { format!("if ({}) {};", cond, body) }
    pub fn expr_stmt(expr: &str) -> String { format!("{};", expr) }
    pub fn sizeof_expr(ty: &str) -> String { format!("sizeof({})", ty) }
    pub fn alignof_expr(ty: &str) -> String { format!("alignof({})", ty) }
    pub fn offsetof_expr(st: &str, field: &str) -> String { format!("offsetof({}, {})", st, field) }
    pub fn static_cast(ty: &str, val: &str) -> String { format!("static_cast<{}>({})", ty, val) }
    /// `Result<void, E>::ok()`
    pub fn result_call(base: &str, args: &str, method: &str, arg: &str) -> String {
        if arg.is_empty() { format!("{}<{}>::{}()", base, args, method) }
        else { format!("{}<{}>::{}({})", base, args, method, arg) }
    }
}
