use std::fmt;

#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub hint: Option<String>,
    /// 出错行的源码文本（用于渲染 caret 指示）
    pub source_line: Option<String>,
    /// 完整 token span：字节区间 [start, end)
    pub span: Option<(usize, usize)>,
}

impl CompileError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self { message: msg.into(), file: None, line: None, column: None,
               hint: None, source_line: None, span: None }
    }
    pub fn with_source_line(mut self, l: impl Into<String>) -> Self {
        self.source_line = Some(l.into());
        self
    }
    pub fn with_span(mut self, span: (usize, usize)) -> Self {
        self.span = Some(span);
        self
    }
    pub fn at(mut self, line: usize, column: usize) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }
    pub fn in_file(mut self, f: impl Into<String>) -> Self {
        self.file = Some(f.into());
        self
    }
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(file) = &self.file { write!(f, "{}:", file)?; }
        if let Some(line) = self.line {
            write!(f, "{}:", line)?;
            if let Some(col) = self.column { write!(f, "{}:", col)?; }
        }
        write!(f, " error: {}", self.message)?;
        // 源码片段：出错行 + 列指示
        if let Some(src_line) = &self.source_line {
            let col = self.column.unwrap_or(1).max(1);
            let caret_pad = " ".repeat(col - 1);
            write!(f, "\n  |\n  | {}\n  | {}^", src_line, caret_pad)?;
        }
        if let Some(hint) = &self.hint {
            write!(f, "\n  hint: {}", hint)?;
        }
        Ok(())
    }
}

impl std::error::Error for CompileError {}

pub type Result<T> = std::result::Result<T, CompileError>;