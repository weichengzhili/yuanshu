//! 统一输出层（write）。
//!
//! 收敛代码中散落的 `println!` / `eprintln!`，统一输出格式：
//! - `out!`       → stdout（用户可见内容：版本号、help、生成报告等）
//! - `progress!`  → stderr，统一前缀 `[idlc]`（进度 / 诊断日志）
//! - `warn!`      → stderr，前缀 `[warn]`
//! - `error!`     → stderr，前缀 `[error]`
//! - `log!`       → stderr，无前缀（通用诊断）
//!
//! 格式前缀集中在本模块的 `Writer` 上，便于统一改名 / 着色 / 重定向。

use std::io::{self, Write};

/// 统一输出器：封装目标流与格式前缀（"类"）。
pub struct Writer;

impl Writer {
    /// stdout：用户可见内容
    pub fn out(&self, msg: &str) {
        let _ = writeln!(io::stdout(), "{}", msg);
    }

    /// stderr：原始诊断（无前缀）
    pub fn raw(&self, msg: &str) {
        let _ = writeln!(io::stderr(), "{}", msg);
    }

    /// 进度日志（[idlc] 前缀）
    pub fn progress(&self, msg: &str) {
        let _ = writeln!(io::stderr(), "[idlc] {}", msg);
    }

    /// 警告（[warn] 前缀）
    pub fn warn(&self, msg: &str) {
        let _ = writeln!(io::stderr(), "[warn] {}", msg);
    }

    /// 错误（[error] 前缀）
    pub fn error(&self, msg: &str) {
        let _ = writeln!(io::stderr(), "[error] {}", msg);
    }
}

#[macro_export]
macro_rules! out {
    ($($arg:tt)*) => { $crate::write::Writer.out(&format!($($arg)*)) };
}

#[macro_export]
macro_rules! progress {
    ($($arg:tt)*) => { $crate::write::Writer.progress(&format!($($arg)*)) };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => { $crate::write::Writer.warn(&format!($($arg)*)) };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => { $crate::write::Writer.error(&format!($($arg)*)) };
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::write::Writer.raw(&format!($($arg)*)) };
}
