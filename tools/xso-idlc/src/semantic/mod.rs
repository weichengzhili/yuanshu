pub mod types;
pub mod version;
pub mod layout;
pub mod mono;

use crate::ast::Module;
use crate::diag::Result;

pub fn analyze(module: &mut Module) -> Result<()> {
    // 1) 泛型单态化：先把泛型展开为具体结构体（下游只看到普通具名类型）
    mono::monomorphize(module)?;
    // 2) 类型/边界检查
    types::check_all(module)?;
    // 3) 版本兼容检查
    version::check_all(module)?;
    Ok(())
}
