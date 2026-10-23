//! 布局对拍测试脚手架：让测试自动校验两端布局一致。
//! 通过 `Emitter` 的 Rust 语义方法产出，本文件不出现写死的 Rust 源码文本。

use crate::bridge_ir::BridgeIr;
use crate::codegen::emitter::Emitter;
use crate::codegen::syntax::rs;

pub fn generate(ir: &BridgeIr) -> String {
    let module = &ir.module;
    let mut em = Emitter::new();

    em.line(&format!("// AUTO-GENERATED layout tests for module '{}'", module.name));
    em.blank();
    em.rs_attr("cfg(test)");
    em.rs_mod_open("layout_tests");

    for (name, layout) in &ir.type_layouts {
        em.rs_attr("test");
        let fn_name = format!("{}_size", name.to_lowercase());
        em.open(&rs::fn_decl(false, "", &fn_name, "", ""));
        em.rs_assert_eq(
            &format!("std::mem::size_of::<super::{}_bridge::{}>()", module.name, name),
            &layout.size.to_string(),
        );
        em.close("}");
    }

    em.rs_mod_close();
    em.finish()
}
