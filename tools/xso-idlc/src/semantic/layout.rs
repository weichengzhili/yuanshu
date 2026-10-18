use crate::ast::{Item, Module, StructDef, TypeRef};

pub struct Layout {
    pub size: usize,
    pub align: usize,
}

pub fn size_of_primitive(name: &str) -> Layout {
    match name {
        "u8" | "i8" | "bool" => Layout { size: 1, align: 1 },
        "u16" | "i16" => Layout { size: 2, align: 2 },
        "u32" | "i32" => Layout { size: 4, align: 4 },
        "u64" | "i64" => Layout { size: 8, align: 8 },
        "unit" => Layout { size: 0, align: 1 },
        _ => Layout { size: 8, align: 8 }, // 保守估计
    }
}

pub fn compute_struct_layout(module: &Module, s: &StructDef) -> Layout {
    let mut size = 0usize;
    let mut align = 1usize;

    for f in &s.fields {
        let fl = layout_of(module, &f.ty);
        // 对齐填充
        size = (size + fl.align - 1) & !(fl.align - 1);
        size += fl.size;
        if fl.align > align { align = fl.align; }
    }
    // 结构体整体对齐
    size = (size + align - 1) & !(align - 1);
    Layout { size, align }
}

pub fn layout_of(module: &Module, ty: &TypeRef) -> Layout {
    match ty {
        TypeRef::Primitive(p) => size_of_primitive(p),
        TypeRef::BorrowedStr => Layout { size: 16, align: 8 }, // ptr + len
        TypeRef::OwnedString => Layout { size: 24, align: 8 }, // ptr + len + cap
        TypeRef::Named(n) => {
            // 找定义
            for item in &module.items {
                match item {
                    Item::Struct(s) if &s.name == n => {
                        return compute_struct_layout(module, s);
                    }
                    Item::Enum(e) if &e.name == n => {
                        return size_of_primitive(&e.underlying);
                    }
                    Item::Error(e) if &e.name == n => {
                        return size_of_primitive(&e.underlying);
                    }
                    Item::TypeAlias(t) if &t.name == n => {
                        return layout_of(module, &t.target);
                    }
                    _ => {}
                }
            }
            Layout { size: 8, align: 8 }
        }
        TypeRef::Vec(_) => Layout { size: 24, align: 8 },   // ptr + len + cap
        TypeRef::Option(inner) => {
            let il = layout_of(module, inner);
            // 判别位 1 字节，加 padding
            let size = (il.size + 1 + il.align - 1) & !(il.align - 1);
            Layout { size, align: il.align }
        }
        TypeRef::Result(ok, err) => {
            let ol = layout_of(module, ok);
            let el = layout_of(module, err);
            let data_size = ol.size.max(el.size);
            let align = ol.align.max(el.align);
            let total = data_size + 4; // 判别 4 字节
            let size = (total + align - 1) & !(align - 1);
            Layout { size, align }
        }
    }
}