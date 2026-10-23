//! 结构化发射器：以语义方法产出目标源码。
//!
//! 后端只调用 `struct_open` / `field` / `assert_eq` / `fn_ptr` 这类**语义方法**，
//! 关键字、标点、缩进、声明模板一律由本发射器 + `syntax` 词汇表掌握，
//! 生成逻辑里不再出现任何写死的源码字符串。

use crate::codegen::syntax;

pub struct Emitter {
    buf: String,
    depth: usize,
}

impl Emitter {
    pub fn new() -> Self { Self { buf: String::new(), depth: 0 } }
    pub fn finish(self) -> String { self.buf }

    fn pad(&self) -> String { "    ".repeat(self.depth) }

    // ===================== 通用 =====================

    /// 注释行（注释前缀由调用方给出，如 `//` 或 `///`）
    pub fn comment(&mut self, prefix: &str, text: &str) {
        self.buf.push_str(&format!("{}{} {}\n", self.pad(), prefix, text));
    }
    /// 无前缀的纯注释（如 `///` 后接空行）
    pub fn comment_raw(&mut self, text: &str) {
        self.buf.push_str(&format!("{}{}\n", self.pad(), text));
    }
    pub fn blank(&mut self) { self.buf.push('\n'); }
    /// 任意文本行（按当前缩进；用于 Markdown / 说明性文本）
    pub fn line(&mut self, s: &str) {
        self.buf.push_str(&format!("{}{}\n", self.pad(), s));
    }

    // ===================== Markdown（文档） =====================

    pub fn md_heading(&mut self, level: usize, text: &str) {
        self.line(&format!("{} {}", "#".repeat(level), text));
    }
    pub fn md_quote(&mut self, text: &str) { self.line(&format!("> {}", text)); }
    /// 表格分隔行：`|---|---|`（n 列）
    pub fn md_table_sep(&mut self, n: usize) {
        let mut row = String::from("|");
        for _ in 0..n { row.push_str("---|"); }
        self.line(&row);
    }
    /// 表格数据行：`| c1 | c2 |`
    pub fn md_table_row(&mut self, cells: &[String]) {
        let mut row = String::from("|");
        for c in cells { row.push_str(&format!(" {} |", c)); }
        self.line(&row);
    }
    /// 块开启：写出头行并增加缩进
    pub fn open(&mut self, header: &str) {
        self.buf.push_str(&format!("{}{}\n", self.pad(), header));
        self.depth += 1;
    }
    /// 块关闭：回缩并写出收尾符号
    pub fn close(&mut self, closer: &str) {
        self.depth = self.depth.saturating_sub(1);
        self.buf.push_str(&format!("{}{}\n", self.pad(), closer));
    }
    /// 语句行（缩进后写出；用于表达式/声明，不含块结构）
    pub fn stmt(&mut self, code: &str) {
        self.buf.push_str(&format!("{}{}\n", self.pad(), code));
    }

    // ===================== Rust =====================

    pub fn rs_comment(&mut self, text: &str) { self.comment("//", text); }
    pub fn rs_doc(&mut self, text: &str) { self.comment(syntax::rs::DOC, text); }
    pub fn rs_allow(&mut self, items: &[&str]) { self.stmt(&syntax::rs::allow(items)); }
    pub fn rs_attr(&mut self, name: &str) { self.stmt(&format!("#[{}]", name)); }
    pub fn rs_extern_crate(&mut self, name: &str) {
        self.stmt(&format!("extern crate {};", name));
    }
    pub fn rs_use(&mut self, path: &str) { self.stmt(&syntax::rs::use_stmt(path)); }
    pub fn rs_repr_c(&mut self) { self.stmt(&syntax::rs::repr_c()); }
    pub fn rs_repr(&mut self, what: &str) { self.stmt(&syntax::rs::repr(what)); }
    pub fn rs_derive(&mut self, items: &[&str]) { self.stmt(&syntax::rs::derive(items)); }

    pub fn rs_mod_open(&mut self, name: &str) { self.open(&syntax::rs::mod_open(name)); }
    pub fn rs_mod_close(&mut self) { self.close("}"); }

    pub fn rs_struct_open(&mut self, name: &str) { self.open(&syntax::rs::struct_open(name)); }
    pub fn rs_struct_close(&mut self) { self.close("}"); }
    pub fn rs_field(&mut self, name: &str, ty: &str) { self.stmt(&syntax::rs::field(name, ty)); }

    pub fn rs_enum_open(&mut self, name: &str) { self.open(&syntax::rs::enum_open(name)); }
    pub fn rs_enum_close(&mut self) { self.close("}"); }
    pub fn rs_variant(&mut self, name: &str, val: i64) {
        self.stmt(&syntax::rs::variant(name, val));
    }

    pub fn rs_impl_open(&mut self, header: &str) {
        self.open(&format!("{} {} {{", syntax::rs::IMPL, header));
    }
    pub fn rs_impl_close(&mut self) { self.close("}"); }

    pub fn rs_trait_open(&mut self, name: &str) { self.open(&syntax::rs::trait_open(name)); }
    pub fn rs_trait_close(&mut self) { self.close("}"); }
    pub fn rs_trait_fn(&mut self, name: &str, params: &str, ret: &str) {
        self.stmt(&syntax::rs::trait_fn(name, params, ret));
    }

    /// 函数声明开启（可见性 + 可选限定符 + 签名）
    pub fn rs_fn_open(&mut self, is_pub: bool, qualifier: &str, name: &str, params: &str, ret: &str) {
        self.open(&syntax::rs::fn_decl(is_pub, qualifier, name, params, ret));
    }
    pub fn rs_fn_close(&mut self) { self.close("}"); }
    /// 单行函数体
    pub fn rs_fn_one_line(
        &mut self, is_pub: bool, qualifier: &str, name: &str, params: &str, ret: &str, body: &str,
    ) {
        self.stmt(&syntax::rs::fn_one_line(is_pub, qualifier, name, params, ret, body));
    }

    pub fn rs_match_open(&mut self, expr: &str) { self.open(&syntax::rs::match_open(expr)); }
    pub fn rs_match_arm(&mut self, pat: &str, body: &str) {
        self.stmt(&syntax::rs::match_arm(pat, body));
    }
    pub fn rs_match_wildcard(&mut self, body: &str) {
        self.stmt(&syntax::rs::match_wildcard(body));
    }
    pub fn rs_match_close(&mut self) { self.close("}"); }

    pub fn rs_const(&mut self, is_pub: bool, name: &str, ty: &str, val: &str) {
        self.stmt(&syntax::rs::const_decl(is_pub, name, ty, val));
    }
    pub fn rs_assert_eq(&mut self, lhs: &str, rhs: &str) {
        self.stmt(&syntax::rs::assert_eq(lhs, rhs));
    }
    pub fn rs_type_alias(&mut self, name: &str, target: &str) {
        self.stmt(&syntax::rs::type_alias(name, target));
    }
    pub fn rs_fn_ptr(&mut self, name: &str, params: &str, ret: &str) {
        self.stmt(&syntax::rs::fn_ptr(name, params, ret));
    }

    // 表达式构造器（供调用方拼语义，而非拼文本）
    pub fn rs_size_of(&self, ty: &str) -> String { syntax::rs::size_of(ty) }
    pub fn rs_align_of(&self, ty: &str) -> String { syntax::rs::align_of(ty) }
    pub fn rs_offset_of(&self, st: &str, field: &str) -> String { syntax::rs::offset_of(st, field) }

    // ===================== C++ =====================

    pub fn cpp_comment(&mut self, text: &str) { self.comment("//", text); }
    pub fn cpp_pragma_once(&mut self) { self.stmt(syntax::cpp::PRAGMA_ONCE); }
    pub fn cpp_include(&mut self, path: &str, quoted: bool) {
        self.stmt(&syntax::cpp::include(path, quoted));
    }
    pub fn cpp_ns_open(&mut self, name: &str) { self.open(&syntax::cpp::namespace_open(name)); }
    pub fn cpp_ns_close(&mut self) { self.close("}"); }

    pub fn cpp_struct_open(&mut self, name: &str) { self.open(&syntax::cpp::struct_open(name)); }
    pub fn cpp_struct_close(&mut self) { self.close(&syntax::cpp::struct_close()); }
    pub fn cpp_member(&mut self, ty: &str, name: &str) {
        self.stmt(&syntax::cpp::member(ty, name));
    }

    pub fn cpp_enum_open(&mut self, name: &str, underlying: &str) {
        self.open(&syntax::cpp::enum_class_open(name, underlying));
    }
    pub fn cpp_enum_close(&mut self) { self.close(&syntax::cpp::enum_class_close()); }
    pub fn cpp_enumerator(&mut self, name: &str, val: i64) {
        self.stmt(&syntax::cpp::enumerator(name, val));
    }

    pub fn cpp_template(&mut self, params: &str) { self.stmt(&syntax::cpp::template(params)); }
    pub fn cpp_static_assert(&mut self, cond: &str, msg: &str) {
        self.stmt(&syntax::cpp::static_assert(cond, msg));
    }
    pub fn cpp_constexpr(&mut self, ty: &str, name: &str, val: &str) {
        self.stmt(&syntax::cpp::constexpr_var(ty, name, val));
    }
    pub fn cpp_class_open(&mut self, name: &str) { self.open(&syntax::cpp::class_open(name)); }
    pub fn cpp_class_close(&mut self) { self.close("};"); }
    pub fn cpp_access(&mut self, spec: &str) { self.stmt(spec); }
    pub fn cpp_fn_open(&mut self, header: &str) { self.open(header); }
    pub fn cpp_fn_close(&mut self) { self.close("}"); }
    pub fn cpp_trivially_copyable(&self, ty: &str) -> String {
        syntax::cpp::trivially_copyable(ty)
    }

    // ===================== anchor =====================

    pub fn anchor_define(&mut self, name: &str, val: &str) {
        self.stmt(&syntax::cpp::define(name, val));
    }
}
