use crate::diag::{CompileError, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    // 声明关键字
    KwModule, KwVersion, KwEnum, KwError, KwStruct, KwInterface,
    KwType, KwConst, KwUse,
    // 注解关键字
    KwAvailable, KwAdded, KwRemoved, KwField, KwMethod,
    // 边界控制关键字
    KwOpaque, KwOwned, KwBorrowed,
    Ident(String),
    Int(i64),
    Str(String),
    At, Colon, Semicolon, Comma, Equals,
    LBrace, RBrace, LParen, RParen,
    LBracket, RBracket, Lt, Gt, Arrow,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
    /// 完整 token span：字节区间 [start, end)
    pub span: (usize, usize),
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self { src: src.as_bytes(), pos: 0, line: 1, col: 1 }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.pos >= self.src.len() { break; }
            // 记录完整 span（不侵入各分支）
            let start = self.pos;
            let mut tok = self.next_token()?;
            tok.span = (start, self.pos);
            tokens.push(tok);
        }
        tokens.push(Token {
            kind: TokenKind::Eof, line: self.line, col: self.col,
            span: (self.pos, self.pos),
        });
        Ok(tokens)
    }

    fn peek(&self) -> Option<u8> { self.src.get(self.pos).copied() }
    fn peek2(&self) -> Option<u8> { self.src.get(self.pos + 1).copied() }

    fn advance(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        if c == b'\n' { self.line += 1; self.col = 1; } else { self.col += 1; }
        Some(c)
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => { self.advance(); }
                Some(b'/') if self.peek2() == Some(b'/') => {
                    while self.peek().map_or(false, |c| c != b'\n') { self.advance(); }
                }
                _ => break,
            }
        }
    }

    fn next_token(&mut self) -> Result<Token> {
        let line = self.line;
        let col = self.col;
        let c = self.peek().unwrap();

        // 标识符起始：ASCII 字母 / '_' / 非 ASCII（UTF-8 多字节首字节，支持 Unicode 标识符）
        if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 {
            let id = self.read_ident();
            let kind = match id.as_str() {
                "module" => TokenKind::KwModule,
                "version" => TokenKind::KwVersion,
                "enum" => TokenKind::KwEnum,
                "error" => TokenKind::KwError,
                "struct" => TokenKind::KwStruct,
                "interface" => TokenKind::KwInterface,
                "type" => TokenKind::KwType,
                "const" => TokenKind::KwConst,
                "use" => TokenKind::KwUse,
                "available" => TokenKind::KwAvailable,
                "added" => TokenKind::KwAdded,
                "removed" => TokenKind::KwRemoved,
                "field" => TokenKind::KwField,
                "method" => TokenKind::KwMethod,
                "opaque" => TokenKind::KwOpaque,
                "owned" => TokenKind::KwOwned,
                "borrowed" => TokenKind::KwBorrowed,
                _ => TokenKind::Ident(id),
            };
            return Ok(Token { kind, line, col, span: (0, 0) });
        }

        if c.is_ascii_digit() {
            let n = self.read_number()?;
            return Ok(Token { kind: TokenKind::Int(n), line, col, span: (0, 0) });
        }

        if c == b'"' {
            let s = self.read_string()?;
            return Ok(Token { kind: TokenKind::Str(s), line, col, span: (0, 0) });
        }

        self.advance();
        let kind = match c {
            b'@' => TokenKind::At,
            b':' => TokenKind::Colon,
            b';' => TokenKind::Semicolon,
            b',' => TokenKind::Comma,
            b'=' => TokenKind::Equals,
            b'{' => TokenKind::LBrace,
            b'}' => TokenKind::RBrace,
            b'(' => TokenKind::LParen,
            b')' => TokenKind::RParen,
            b'[' => TokenKind::LBracket,
            b']' => TokenKind::RBracket,
            b'<' => TokenKind::Lt,
            b'>' => TokenKind::Gt,
            b'-' if self.peek() == Some(b'>') => { self.advance(); TokenKind::Arrow }
            _ => return Err(CompileError::new(format!("unexpected char '{}'", c as char)).at(line, col)),
        };
        Ok(Token { kind, line, col, span: (0, 0) })
    }

    fn read_ident(&mut self) -> String {
        let start = self.pos;
        // 标识符续接：ASCII 字母数字 / '_' / 非 ASCII（UTF-8 续字节）
        while self.peek().map_or(false, |c| c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80) {
            self.advance();
        }
        String::from_utf8_lossy(&self.src[start..self.pos]).to_string()
    }

    fn read_number(&mut self) -> Result<i64> {
        let line = self.line;
        let col = self.col;
        if self.peek() == Some(b'0') && self.peek2().map_or(false, |c| c == b'x' || c == b'X') {
            self.advance(); self.advance();
            let s = self.pos;
            while self.peek().map_or(false, |c| c.is_ascii_hexdigit()) { self.advance(); }
            let txt = std::str::from_utf8(&self.src[s..self.pos]).unwrap();
            i64::from_str_radix(txt, 16).map_err(|_| CompileError::new("invalid hex").at(line, col))
        } else {
            let s = self.pos;
            while self.peek().map_or(false, |c| c.is_ascii_digit()) { self.advance(); }
            let txt = std::str::from_utf8(&self.src[s..self.pos]).unwrap();
            txt.parse::<i64>().map_err(|_| CompileError::new("invalid number").at(line, col))
        }
    }

    fn read_string(&mut self) -> Result<String> {
        self.advance();
        let s = self.pos;
        while self.peek().map_or(false, |c| c != b'"') { self.advance(); }
        let txt = String::from_utf8_lossy(&self.src[s..self.pos]).to_string();
        self.advance();
        Ok(txt)
    }
}