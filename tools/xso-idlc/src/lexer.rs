use crate::diag::{CompileError, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    // 声明关键字
    KwModule, KwVersion, KwEnum, KwError, KwStruct, KwInterface,
    KwType, KwConst, KwUse, KwAs,
    // 注解关键字
    KwAvailable, KwAdded, KwRemoved, KwField, KwMethod,
    // 边界控制关键字
    KwOpaque, KwOwned, KwBorrowed,
    Ident(String),
    Int(i64),
    Str(String),
    At, Colon, ColonColon, Semicolon, Comma, Equals,
    LBrace, RBrace, LParen, RParen,
    LBracket, RBracket, Lt, Gt, Arrow,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        // 容忍 UTF-8 BOM（Windows 编辑器常见）：跳过前导 EF BB BF，避免误报 "unexpected char"
        let bytes = src.as_bytes();
        let start = if bytes.len() >= 3 && bytes[..3] == [0xEF, 0xBB, 0xBF] { 3 } else { 0 };
        Self { src: bytes, pos: start, line: 1, col: 1 }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.pos >= self.src.len() { break; }
            tokens.push(self.next_token()?);
        }
        tokens.push(Token { kind: TokenKind::Eof, line: self.line, col: self.col });
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

        if self.peek_is_ident_start() {
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
                "as" => TokenKind::KwAs,
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
            return Ok(Token { kind, line, col });
        }

        if c.is_ascii_digit() {
            let n = self.read_number()?;
            return Ok(Token { kind: TokenKind::Int(n), line, col });
        }

        if c == b'"' {
            let s = self.read_string()?;
            return Ok(Token { kind: TokenKind::Str(s), line, col });
        }

        self.advance();
        let kind = match c {
            b'@' => TokenKind::At,
            b':' if self.peek() == Some(b':') => { self.advance(); TokenKind::ColonColon }
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
        Ok(Token { kind, line, col })
    }

    /// 标识符起点：ASCII 字母/下划线，或任意 Unicode XID 起始字符（支持中文等）
    fn peek_is_ident_start(&self) -> bool {
        match std::str::from_utf8(&self.src[self.pos..]) {
            Ok(s) => match s.chars().next() {
                Some(c) => c == '_' || c.is_alphabetic(),
                None => false,
            },
            Err(_) => false,
        }
    }

    fn read_ident(&mut self) -> String {
        let start = self.pos;
        let rest = match std::str::from_utf8(&self.src[self.pos..]) {
            Ok(r) => r,
            Err(_) => return String::new(),
        };
        let mut consumed = 0usize;
        for (i, ch) in rest.char_indices() {
            if i == 0 {
                if !(ch == '_' || ch.is_alphabetic()) { break; }
            } else if !(ch == '_' || ch.is_alphanumeric()) {
                break;
            }
            consumed = i + ch.len_utf8();
        }
        while self.pos < start + consumed { self.advance(); }
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