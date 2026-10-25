use crate::ast::{Item, Module, StructDef, TypeRef, Ownership};

#[derive(Clone, Copy, Debug)]
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
    // 生成的结构体恒为 #[repr(C, packed)] / __attribute__((packed))，
    // 不做对齐填充，整体对齐为 1。
    let mut size = 0usize;
    let mut align = 1usize;
    for f in &s.fields {
        let fl = layout_of(module, &f.ty);
        size = fl.align_up(size);
        size += fl.size;
        if fl.align > align { align = fl.align; }
    }
    Layout { size, align: 1 }
}

pub fn compute_field_offsets(module: &Module, s: &StructDef) -> Vec<(String, usize)> {
    let mut offsets = Vec::new();
    let mut offset = 0usize;
    for f in &s.fields {
        let fl = layout_of(module, &f.ty);
    // packed：字段紧密排列，无对齐填充
        offset = fl.align_up(offset);
        offsets.push((f.name.clone(), offset));
        offset += fl.size;
    }
    offsets
}

pub fn layout_of(module: &Module, ty: &TypeRef) -> Layout {
    match ty {
        TypeRef::Primitive(p) => size_of_primitive(p),
        TypeRef::BorrowedStr => Layout { size: 16, align: 8 },
        TypeRef::OwnedString => Layout { size: 24, align: 8 },
        TypeRef::Owned(_) | TypeRef::Borrowed(_) | TypeRef::App { .. } => Layout::PTR,
        TypeRef::Vec(_) | TypeRef::Set(_) | TypeRef::Map(_, _) => Layout { size: 16, align: 8 },
        TypeRef::Option(_) => Layout::PTR,
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