use crate::ast::*;
use crate::diag::{CompileError, Result};
use crate::lexer::{Lexer, Token, TokenKind};

pub fn parse(src: &str) -> Result<Module> {
    let tokens = Lexer::new(src).tokenize()?;
    let mut parser = Parser { tokens, pos: 0 };
    parser.parse_module()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &TokenKind { &self.tokens[self.pos].kind }
    fn peek_tok(&self) -> &Token { &self.tokens[self.pos] }

    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() { self.pos += 1; }
        t
    }

    fn check(&self, k: &TokenKind) -> bool { self.peek() == k }

    fn expect(&mut self, k: &TokenKind, what: &str) -> Result<Token> {
        if self.check(k) {
            Ok(self.advance())
        } else {
            let t = self.peek_tok();
            Err(CompileError::new(format!(
                "expected {}, got {:?}", what, t.kind
            )).at(t.line, t.col))
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.peek().clone() {
            TokenKind::Ident(s) => { self.advance(); Ok(s) }
            other => {
                let t = self.peek_tok();
                Err(CompileError::new(format!(
                    "expected identifier, got {:?}", other
                )).at(t.line, t.col))
            }
        }
    }

    /// 读取标识符，注解位置上关键字也视为标识符（`@module` / `@field` / `added=1` 等）
    fn expect_ident_or_kw(&mut self) -> Result<String> {
        let tok = self.peek().clone();
        let s: String = match tok {
            TokenKind::Ident(s) => s,
            TokenKind::KwModule    => "module".into(),
            TokenKind::KwVersion   => "version".into(),
            TokenKind::KwEnum      => "enum".into(),
            TokenKind::KwError     => "error".into(),
            TokenKind::KwStruct    => "struct".into(),
            TokenKind::KwInterface => "interface".into(),
            TokenKind::KwType      => "type".into(),
            TokenKind::KwConst     => "const".into(),
            TokenKind::KwUse       => "use".into(),
            TokenKind::KwAvailable => "available".into(),
            TokenKind::KwAdded     => "added".into(),
            TokenKind::KwRemoved   => "removed".into(),
            TokenKind::KwField     => "field".into(),
            TokenKind::KwMethod    => "method".into(),
            other => {
                let t = self.peek_tok();
                return Err(CompileError::new(format!(
                    "expected identifier, got {:?}", other
                )).at(t.line, t.col));
            }
        };
        self.advance();
        Ok(s)
    }
    /// 把当前 token 当作标识符字面量取出（关键字回退成文本），失败返回 None
    fn take_ident_or_kw(&mut self) -> Option<String> {
        match self.peek().clone() {
            TokenKind::Ident(s) => { self.advance(); Some(s) }
            other => {
                let s = other.keyword_text()?.to_string();
                self.advance();
                Some(s)
            }
        }
    }

    fn expect_int(&mut self) -> Result<i64> {
        match self.peek().clone() {
            TokenKind::Int(n) => { self.advance(); Ok(n) }
            other => {
                let t = self.peek_tok();
                Err(CompileError::new(format!(
                    "expected integer, got {:?}", other
                )).at(t.line, t.col))
            }
        }
    }

    // ---- 顶层 ----
    fn parse_module(&mut self) -> Result<Module> {
        let mut name = String::from("unnamed");
        let mut version: u32 = 1;

        while self.check(&TokenKind::At) {
            let ann = self.parse_annotation()?;
            match ann.name.as_str() {
                "module" => {
                    if let Some(n) = ann.ident_arg(0) {
                        name = n.to_string();
                    }
                }
                "version" => {
                    if let Some(v) = ann.int_arg(0) {
                        version = v as u32;
                    }
                }
                _ => {}
            }
        }

        let mut items = Vec::new();
        while !self.check(&TokenKind::Eof) {
            items.push(self.parse_item()?);
        }

        Ok(Module { name, version, items })
    }

    fn parse_item(&mut self) -> Result<Item> {
        let _anns = self.parse_annotations()?;

        match self.peek().clone() {
            TokenKind::KwEnum => Ok(Item::Enum(self.parse_enum()?)),
            TokenKind::KwError => Ok(Item::Error(self.parse_error()?)),
            TokenKind::KwStruct => Ok(Item::Struct(self.parse_struct()?)),
            TokenKind::KwInterface => Ok(Item::Interface(self.parse_interface()?)),
            TokenKind::KwType => Ok(Item::TypeAlias(self.parse_type_alias()?)),
            TokenKind::KwUse => {
                self.skip_use()?;
                // use 不产生 item，递归解析下一个
                if self.check(&TokenKind::Eof) {
                    Err(CompileError::new("use at end of file"))
                } else {
                    self.parse_item()
                }
            }
            other => {
                let t = self.peek_tok();
                Err(CompileError::new(format!(
                    "expected item (enum/error/struct/interface/type), got {:?}", other
                )).at(t.line, t.col))
            }
        }
    }

    fn skip_use(&mut self) -> Result<()> {
        self.advance(); // use
        // 吃掉到 ; 为止
        while !self.check(&TokenKind::Semicolon) && !self.check(&TokenKind::Eof) {
            self.advance();
        }
        if self.check(&TokenKind::Semicolon) { self.advance(); }
        Ok(())
    }

    // ---- enum / error ----
    fn parse_enum(&mut self) -> Result<EnumDef> {
        self.expect(&TokenKind::KwEnum, "'enum'")?;
        let name = self.expect_ident()?;
        let underlying = if self.check(&TokenKind::Colon) {
            self.advance();
            self.expect_ident()?
        } else {
            "u32".to_string()
        };
        self.expect(&TokenKind::LBrace, "'{'")?;
        let mut variants = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let anns = self.parse_annotations()?;
            let vname = self.expect_ident()?;
            self.expect(&TokenKind::Equals, "'='")?;
            let val = self.expect_int()?;
            variants.push(Variant { name: vname, value: val, annotations: anns });
            if self.check(&TokenKind::Comma) { self.advance(); }
        }
        self.expect(&TokenKind::RBrace, "'}'")?;
        Ok(EnumDef { name, underlying, variants })
    }

    fn parse_error(&mut self) -> Result<ErrorDef> {
        self.expect(&TokenKind::KwError, "'error'")?;
        let name = self.expect_ident()?;
        let underlying = if self.check(&TokenKind::Colon) {
            self.advance();
            self.expect_ident()?
        } else {
            "i32".to_string()
        };
        self.expect(&TokenKind::LBrace, "'{'")?;
        let mut variants = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let anns = self.parse_annotations()?;
            let vname = self.expect_ident()?;
            self.expect(&TokenKind::Equals, "'='")?;
            let val = self.expect_int()?;
            variants.push(Variant { name: vname, value: val, annotations: anns });
            if self.check(&TokenKind::Comma) { self.advance(); }
        }
        self.expect(&TokenKind::RBrace, "'}'")?;
        Ok(ErrorDef { name, underlying, variants })
    }

    // ---- struct ----
    fn parse_struct(&mut self) -> Result<StructDef> {
        self.expect(&TokenKind::KwStruct, "'struct'")?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace, "'{'")?;
        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let anns = self.parse_annotations()?;
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Colon, "':'")?;
            let ty = self.parse_type()?;
            fields.push(Field { name: fname, ty, annotations: anns });
            if self.check(&TokenKind::Comma) { self.advance(); }
        }
        self.expect(&TokenKind::RBrace, "'}'")?;
        Ok(StructDef { name, fields })
    }

    // ---- interface ----
    fn parse_interface(&mut self) -> Result<InterfaceDef> {
        self.expect(&TokenKind::KwInterface, "'interface'")?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace, "'{'")?;
        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let anns = self.parse_annotations()?;
            let mname = self.expect_ident()?;
            self.expect(&TokenKind::LParen, "'('")?;
            let mut params = Vec::new();
            while !self.check(&TokenKind::RParen) {
                let pname = self.expect_ident()?;
                self.expect(&TokenKind::Colon, "':'")?;
                let pty = self.parse_type()?;
                params.push(Param { name: pname, ty: pty });
                if self.check(&TokenKind::Comma) { self.advance(); }
            }
            self.expect(&TokenKind::RParen, "')'")?;
            self.expect(&TokenKind::Arrow, "'->'")?;
            let ret = self.parse_type()?;
            self.expect(&TokenKind::Semicolon, "';'")?;
            methods.push(Method {
                name: mname, params, ret, annotations: anns,
            });
        }
        self.expect(&TokenKind::RBrace, "'}'")?;
        Ok(InterfaceDef { name, methods })
    }

    // ---- type alias ----
    fn parse_type_alias(&mut self) -> Result<TypeAliasDef> {
        self.expect(&TokenKind::KwType, "'type'")?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Equals, "'='")?;
        let target = self.parse_type()?;
        self.expect(&TokenKind::Semicolon, "';'")?;
        Ok(TypeAliasDef { name, target })
    }

    // ---- type ----
    fn parse_type(&mut self) -> Result<TypeRef> {
        // 忽略 @owned / @borrowed 前缀
        if self.check(&TokenKind::At) {
            self.advance();
            let _ = self.take_ident_or_kw().ok_or_else(|| {
                let t = self.peek_tok();
                CompileError::new(format!("expected annotation name, got {:?}", t.kind))
                    .at(t.line, t.col)
            })?;
        }

        let name = self.expect_ident()?;

        match name.as_str() {
            "Vec" | "vec" => {
                self.expect(&TokenKind::Lt, "'<'")?;
                let inner = self.parse_type()?;
                self.expect(&TokenKind::Gt, "'>'")?;
                Ok(TypeRef::Vec(Box::new(inner)))
            }
            "Option" | "option" => {
                self.expect(&TokenKind::Lt, "'<'")?;
                let inner = self.parse_type()?;
                self.expect(&TokenKind::Gt, "'>'")?;
                Ok(TypeRef::Option(Box::new(inner)))
            }
            "result" | "Result" => {
                self.expect(&TokenKind::Lt, "'<'")?;
                let ok = self.parse_type()?;
                self.expect(&TokenKind::Comma, "','")?;
                let err = self.parse_type()?;
                self.expect(&TokenKind::Gt, "'>'")?;
                Ok(TypeRef::Result(Box::new(ok), Box::new(err)))
            }
            "BorrowedStr" => Ok(TypeRef::BorrowedStr),
            "OwnedString" | "String" => Ok(TypeRef::OwnedString),
            "u8" | "u16" | "u32" | "u64"
            | "i8" | "i16" | "i32" | "i64"
            | "bool" | "unit" => Ok(TypeRef::Primitive(name)),
            _ => Ok(TypeRef::Named(name)),
        }
    }

    // ---- annotations ----
    fn parse_annotations(&mut self) -> Result<Vec<Annotation>> {
        let mut v = Vec::new();
        while self.check(&TokenKind::At) {
            v.push(self.parse_annotation()?);
        }
        Ok(v)
    }

    fn parse_annotation(&mut self) -> Result<Annotation> {
        self.expect(&TokenKind::At, "'@'")?;
        let name = self.expect_ident_or_kw()?;
        let mut args = Vec::new();
        if self.check(&TokenKind::LParen) {
            self.advance();
            while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                // 先看看是不是 key=value
                let first = match self.peek().clone() {
                    TokenKind::Ident(s) => { self.advance(); AnnotationArg::Ident(s) }
                    TokenKind::Int(n) => { self.advance(); AnnotationArg::Int(n) }
                    TokenKind::Str(s) => { self.advance(); AnnotationArg::Str(s) }
                    _ => match self.take_ident_or_kw() {
                        // 关键字（added / removed / type ...）当普通标识符
                        Some(s) => AnnotationArg::Ident(s),
                        None => {
                            let t = self.peek_tok();
                            return Err(CompileError::new(format!(
                                "unexpected annotation arg: {:?}", t.kind
                            )).at(t.line, t.col));
                        }
                    }
                };
                if self.check(&TokenKind::Equals) {
                    self.advance();
                    let key = match first {
                        AnnotationArg::Ident(s) => s,
                        _ => return Err(CompileError::new("annotation key must be ident")),
                    };
                    let val = match self.peek().clone() {
                        TokenKind::Int(n) => { self.advance(); AnnotationArg::Int(n) }
                        TokenKind::Ident(s) => { self.advance(); AnnotationArg::Ident(s) }
                        TokenKind::Str(s) => { self.advance(); AnnotationArg::Str(s) }
                        _ => match self.take_ident_or_kw() {
                            Some(s) => AnnotationArg::Ident(s),
                            None => {
                                let t = self.peek_tok();
                                return Err(CompileError::new(format!(
                                    "unexpected annotation value: {:?}", t.kind
                                )).at(t.line, t.col));
                            }
                        }
                    };
                    args.push((Some(key), val));
                } else {
                    args.push((None, first));
                }
                if self.check(&TokenKind::Comma) { self.advance(); }
            }
            self.expect(&TokenKind::RParen, "')'")?;
        }
        Ok(Annotation { name, args })
    }
}