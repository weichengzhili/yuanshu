use std::fmt;

#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub hint: Option<String>,
}

impl CompileError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self { message: msg.into(), file: None, line: None, column: None, hint: None }
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
        if let Some(hint) = &self.hint {
            write!(f, "\n  hint: {}", hint)?;
        }
        Ok(())
    }
}

impl std::error::Error for CompileError {}

pub type Result<T> = std::result::Result<T, CompileError>;