use crate::ast::{Item, Module, StructDef, TypeRef, Ownership};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub size: usize,
    pub align: usize,
}

impl Layout {
    pub const ZERO: Layout = Layout { size: 0, align: 1 };
    pub const PTR:  Layout = Layout { size: 8, align: 8 };

    pub fn align_up(&self, offset: usize) -> usize {
        (offset + self.align - 1) & !(self.align - 1)
    }
}

pub fn size_of_primitive(name: &str) -> Layout {
    match name {
        "u8" | "i8" | "bool" => Layout { size: 1, align: 1 },
        "u16" | "i16" => Layout { size: 2, align: 2 },
        "u32" | "i32" => Layout { size: 4, align: 4 },
        "u64" | "i64" => Layout { size: 8, align: 8 },
        "unit" => Layout { size: 0, align: 1 },
        _ => Layout::PTR,
    }
}

pub fn compute_struct_layout(module: &Module, s: &StructDef) -> Layout {
    // 生成的结构体为 #[repr(C)]（自然对齐），与 C++ 端逐字段同序即同布局。
    let mut size = 0usize;
    let mut align = 1usize;
    for f in &s.fields {
        let fl = layout_of(module, &f.ty);
        size = fl.align_up(size);
        size += fl.size;
        if fl.align > align { align = fl.align; }
    }
    size = (size + align - 1) & !(align - 1);
    Layout { size, align }
}

pub fn compute_field_offsets(module: &Module, s: &StructDef) -> Vec<(String, usize)> {
    let mut offsets = Vec::new();
    let mut offset = 0usize;
    for f in &s.fields {
        let fl = layout_of(module, &f.ty);
        offset = fl.align_up(offset);
        offsets.push((f.name.clone(), offset));
        offset += fl.size;
    }
    offsets
}

pub fn layout_of(module: &Module, ty: &TypeRef) -> Layout {
    match ty {
        TypeRef::Primitive(p) => size_of_primitive(p),
        // 泛型应用：按单态名解析（单态化后应已改写为 Named）
        TypeRef::App { name, args } =>
            layout_of(module, &TypeRef::Named(crate::semantic::mono::mangle(name, args))),
        // 以下类型在 FFI 边界上收敛为「中间布局」的规范载体（与语言原生容器脱钩）：
        //   字符串   -> XsoVec<u8>   (ptr, len)
        //   序列     -> XsoVec<T>    (ptr, len)
        //   关联/集合 -> XsoVec<..>  (ptr, len)
        //   可选     -> *const T     (裸指针，None = null)
        //   所有权   -> *mut/*const T
        TypeRef::BorrowedStr => Layout { size: 16, align: 8 },
        TypeRef::OwnedString => Layout { size: 16, align: 8 },
        // @owned -> XsoBox<T>（ptr + drop 壳）= 16/8；@borrowed 仍是裸指针
        TypeRef::Owned(_) => Layout { size: 16, align: 8 },
        TypeRef::Borrowed(_) => Layout::PTR,
        TypeRef::Vec(_) | TypeRef::Set(_) | TypeRef::Map(_, _) => Layout { size: 16, align: 8 },
        // Option -> XsoOption<T>：#[repr(C)] { u8 tag; T value; }
        TypeRef::Option(inner) => {
            let il = layout_of(module, inner);
            let align = il.align.max(1);
            let size = ((1 + align - 1) / align) * align + il.size;
            Layout { size: (size + align - 1) / align * align, align }
        }
        TypeRef::Result(ok, err) => {
            let ol = layout_of(module, ok);
            let el = layout_of(module, err);
            let data = ol.size.max(el.size);
            let align = ol.align.max(el.align);
            let size = (data + 4 + align - 1) & !(align - 1);
            Layout { size, align }
        }
        TypeRef::Named(n) => {
            // 不透明类型：只通过指针传递
            if module.is_opaque(n) { return Layout::PTR; }
            for item in &module.items {
                match item {
                    Item::Struct(s) if &s.name == n => return compute_struct_layout(module, s),
                    Item::Enum(e) if &e.name == n => return size_of_primitive(&e.underlying),
                    Item::Error(e) if &e.name == n => return size_of_primitive(&e.underlying),
                    Item::TypeAlias(t) if &t.name == n => return layout_of(module, &t.target),
                    _ => {}
                }
            }
            Layout::PTR
        }
    }
}

/// 检查类型引用是否按值传递（用于 opaque 校验）
pub fn is_by_value(_module: &Module, ty: &TypeRef, own: Ownership) -> bool {
    if own != Ownership::Value { return false; }
    match ty {
        TypeRef::Borrowed(_) | TypeRef::Owned(_) => false,
        _ => true,
    }
}