//! 桥接词汇表：所有跨语言名称 / 前缀 / 命名规则的**单一真相**。
//!
//! 代码生成各后端（rust / cpp / anchor / doc）一律从这里取名字，
//! 禁止在生成逻辑里散落硬编码字符串，便于统一改名与主题化定制。

// ===== C++ 侧：内核提供的易用容器/类型 =====
/// 生成代码中 IR 版本常量的标识符名（如 `constexpr uint32_t IR_VERSION = ...`）
pub const IR_VERSION: &str = "IR_VERSION";

pub const CXX_VECTOR: &str = "xso::Vector";
pub const CXX_OPTIONAL: &str = "xso::Optional";
pub const CXX_UNIQUE_PTR: &str = "xso::UniquePtr";
pub const CXX_STRING: &str = "xso::String";
pub const CXX_STRING_VIEW: &str = "xso::string_view";
pub const CXX_RESULT: &str = "xso::Result";

// ===== 中间布局：规范桥接载体 =====
pub const VEC: &str = "XsoVec";
pub const PAIR: &str = "XsoPair";
pub const BOX: &str = "XsoBox";
pub const OPTION: &str = "XsoOption";

/// 规范载体（XsoVec）的固定布局：{ ptr, len } —— 两端必须一致
pub const CARRIER_SIZE: usize = 16;
pub const CARRIER_ALIGN: usize = 8;

// ===== anchor（布局唯一真相）常量前缀 =====
pub const LAYOUT_PREFIX: &str = "XSO_LAYOUT_";
pub const CHECKSUM_PREFIX: &str = "XSO_ABI_CHECKSUM_";
pub const ALGO_VERSION: &str = "XSO_FINGERPRINT_ALGO_VERSION";
pub const IR_VERSION_PREFIX: &str = "XSO_IR_VERSION_";
pub const ANCHOR_HEADER: &str = "layout_anchor.h";

// ===== 生成物中的辅助函数名 =====
pub const FROM_CODE_SUFFIX: &str = "_from_code";
pub const BOUNDARY_VIOLATION: &str = "xso_boundary_violation";

/// C++ 命名空间：`xso::<module>::bridge`
pub fn cxx_namespace(module: &str) -> String {
    format!("xso::{}::bridge", module)
}

/// Rust 模块名：`<module>_bridge`
pub fn rust_module(module: &str) -> String {
    format!("{}_bridge", module)
}

/// 判别式安全转换函数名：`<Enum>_from_code`
pub fn from_code_fn(enum_name: &str) -> String {
    format!("{}{}", enum_name, FROM_CODE_SUFFIX)
}

/// anchor 的 size 宏名：`XSO_LAYOUT_<N>_SIZE`
pub fn layout_size_macro(name: &str) -> String {
    format!("{}{}_SIZE", LAYOUT_PREFIX, name)
}

/// anchor 的 align 宏名：`XSO_LAYOUT_<N>_ALIGN`
pub fn layout_align_macro(name: &str) -> String {
    format!("{}{}_ALIGN", LAYOUT_PREFIX, name)
}

/// anchor 的字段偏移宏名：`XSO_LAYOUT_<N>_OFF_<F>`
pub fn layout_offset_macro(name: &str, field: &str) -> String {
    format!("{}{}_OFF_{}", LAYOUT_PREFIX, name, field)
}

/// Rust 侧的布局常量名：`LAYOUT_<N>_SIZE`
pub fn rust_layout_const(name: &str, kind: &str) -> String {
    format!("LAYOUT_{}_{}", name, kind)
}

/// 规范载体全名：`XsoVec<u8>` / `XsoPair<K,V>` / `XsoBox<T>`
pub fn carrier(base: &str, args: &str) -> String {
    if args.is_empty() { base.to_string() } else { format!("{}<{}>", base, args) }
}

/// ABI 校验和常量名：`XSO_ABI_CHECKSUM_<MODULE>`
pub fn checksum_const(module_up: &str) -> String {
    format!("{}{}", CHECKSUM_PREFIX, module_up)
}

/// IR 版本常量名：`XSO_IR_VERSION_<MODULE>`
pub fn ir_version_const(module_up: &str) -> String {
    format!("{}{}", IR_VERSION_PREFIX, module_up)
}
