pub mod types;
pub mod version;
pub mod layout;

use crate::ast::{ Module};
use crate::diag::Result;

pub fn analyze(module: &Module) -> Result<()> {
    types::check_all(module)?;
    version::check_all(module)?;
    Ok(())
}