use crate::bridge_ir::BridgeIr;
use std::fmt::Write;

/// 生成布局对拍测试：让测试自动校验两端布局
pub fn generate(ir: &BridgeIr) -> String {
    let mut out = String::new();
    let module = &ir.module;

    writeln!(out, "// AUTO-GENERATED layout tests for module '{}'", module.name).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "#[cfg(test)]").unwrap();
    writeln!(out, "mod layout_tests {{").unwrap();

    for (name, layout) in &ir.type_layouts {
        writeln!(out, "    #[test]").unwrap();
        writeln!(out, "    fn {}_size() {{", name.to_lowercase()).unwrap();
        writeln!(out, "        assert_eq!(std::mem::size_of::<super::{}_bridge::{}>(), {});",
                 module.name, name, layout.size).unwrap();
        writeln!(out, "    }}").unwrap();
    }

    writeln!(out, "}}").unwrap();
    out
}