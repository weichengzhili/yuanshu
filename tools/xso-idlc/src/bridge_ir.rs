use std::collections::{HashMap, HashSet};
use crate::ast::*;
use crate::semantic::layout::{self, Layout};

pub struct BridgeIr {
    pub module: Module,
    /// 共享类型的布局
    pub type_layouts: HashMap<String, Layout>,
    /// 共享结构体的字段偏移（有序）
    pub field_offsets: HashMap<String, Vec<(String, Layout, usize)>>,
    /// 枚举/错误底层大小
    pub enum_sizes: HashMap<String, Layout>,
    /// 不透明类型集合
    pub opaque_types: HashSet<String>,
}

impl BridgeIr {
    pub fn build(module: &Module) -> Self {
        let mut ir = Self {
            module: module.clone(),
            type_layouts: HashMap::new(),
            field_offsets: HashMap::new(),
            enum_sizes: HashMap::new(),
            opaque_types: HashSet::new(),
        };

        // 第一遍：识别 opaque
        for item in &module.items {
            match item {
                Item::Struct(s) if s.opaque => { ir.opaque_types.insert(s.name.clone()); }
                Item::Interface(i) => {
                    // 接口天然不透明
                    ir.opaque_types.insert(i.name.clone());
                }
                _ => {}
            }
        }

        // 第二遍：计算共享类型布局
        for item in &module.items {
            match item {
                Item::Struct(s) if !s.opaque => {
                    let l = layout::compute_struct_layout(module, s);
                    ir.type_layouts.insert(s.name.clone(), l);

                    let mut field_info = Vec::new();
                    let mut offset = 0usize;
                    for f in &s.fields {
                        let fl = layout::layout_of(module, &f.ty);
                        offset = fl.align_up(offset);
                        field_info.push((f.name.clone(), fl, offset));
                        offset += fl.size;
                    }
                    ir.field_offsets.insert(s.name.clone(), field_info);
                }
                Item::Enum(e) => {
                    ir.enum_sizes.insert(e.name.clone(),
                                         layout::size_of_primitive(&e.underlying));
                }
                Item::Error(e) => {
                    ir.enum_sizes.insert(e.name.clone(),
                                         layout::size_of_primitive(&e.underlying));
                }
                _ => {}
            }
        }

        ir
    }

    pub fn is_opaque(&self, name: &str) -> bool {
        self.opaque_types.contains(name)
    }

    pub fn layout_of(&self, name: &str) -> Option<Layout> {
        self.type_layouts.get(name).copied()
    }

    pub fn enum_layout(&self, name: &str) -> Option<Layout> {
        self.enum_sizes.get(name).copied()
    }

    pub fn field_offsets(&self, name: &str) -> Option<&[(String, Layout, usize)]> {
        self.field_offsets.get(name).map(|v| v.as_slice())
    }
}