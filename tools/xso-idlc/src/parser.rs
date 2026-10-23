use crate::ast::*;
use crate::diag::{CompileError, Result};
use crate::lexer::{Lexer, Token, TokenKind};

/// 记号数上限（资源限制，防超大/恶意输入）
pub const MAX_TOKENS: usize = 200_000;

pub fn parse(src: &str) -> Result<Module> {
    let tokens = Lexer::new(src).tokenize()?;
    if tokens.len() > MAX_TOKENS {
        return Err(CompileError::new(format!(
            "too many tokens: {} (limit {})", tokens.len(), MAX_TOKENS)));
    }
    let mut parser = Parser { tokens, pos: 0, errors: Vec::new() };
    parser.parse_module()
}

/// 带回退恢复的解析：一次收集全部语法错误（不因第一个错误停止）
pub fn parse_with_recovery(src: &str) -> (Option<Module>, Vec<CompileError>) {
    let tokens = match Lexer::new(src).tokenize() {
        Ok(t) => t,
        Err(e) => return (None, vec![e]),
    };
    if tokens.len() > MAX_TOKENS {
        return (None, vec![CompileError::new(format!(
            "too many tokens: {} (limit {})", tokens.len(), MAX_TOKENS))]);
    }
    let mut parser = Parser { tokens, pos: 0, errors: Vec::new() };
    match parser.parse_module() {
        Ok(m) => (Some(m), parser.errors),
        Err(e) => { parser.errors.push(e); (None, parser.errors) }
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// 错误恢复模式下收集的全部错误
    errors: Vec<CompileError>,
}

/// 是否为顶层条目边界（可作恢复点）
fn is_item_boundary(k: &TokenKind) -> bool {
    matches!(k,
        TokenKind::At | TokenKind::KwEnum | TokenKind::KwError | TokenKind::KwStruct
        | TokenKind::KwInterface | TokenKind::KwType | TokenKind::KwConst | TokenKind::KwUse)
}

/// 是否为成员边界（字段/方法/变体，可作恢复点）
fn is_member_boundary(k: &TokenKind) -> bool {
    is_item_boundary(k) || matches!(k,
        TokenKind::Comma | TokenKind::Semicolon | TokenKind::RBrace)
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
        if self.check(k) { Ok(self.advance()) }
        else {
            let t = self.peek_tok();
            Err(CompileError::new(format!("expected {}, got {:?}", what, t.kind))
                .at(t.line, t.col).with_span(t.span))
        }
    }

    /// 接受关键字和标识符（修复 @module 报错）
    fn expect_ident_or_kw(&mut self) -> Result<String> {
        let tok = self.peek().clone();
        let s = match tok {
            TokenKind::Ident(s) => s,
            TokenKind::KwModule => "module".into(),
            TokenKind::KwVersion => "version".into(),
            TokenKind::KwEnum => "enum".into(),
            TokenKind::KwError => "error".into(),
            TokenKind::KwStruct => "struct".into(),
            TokenKind::KwInterface => "interface".into(),
            TokenKind::KwType => "type".into(),
            TokenKind::KwConst => "const".into(),
            TokenKind::KwUse => "use".into(),
            TokenKind::KwAvailable => "available".into(),
            TokenKind::KwAdded => "added".into(),
            TokenKind::KwRemoved => "removed".into(),
            TokenKind::KwField => "field".into(),
            TokenKind::KwMethod => "method".into(),
            TokenKind::KwOpaque => "opaque".into(),
            TokenKind::KwOwned => "owned".into(),
            TokenKind::KwBorrowed => "borrowed".into(),
            other => {
                let t = self.peek_tok();
                return Err(CompileError::new(format!(
                    "expected identifier, got {:?}", other
                )).at(t.line, t.col).with_span(t.span));
            }
        };
        self.advance();
        Ok(s)
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.peek().clone() {
            TokenKind::Ident(s) => { self.advance(); Ok(s) }
            other => {
                let t = self.peek_tok();
                Err(CompileError::new(format!(
                    "expected identifier, got {:?}", other
                )).at(t.line, t.col).with_span(t.span))
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
                )).at(t.line, t.col).with_span(t.span))
            }
        }
    }

    /// 恐慌模式同步：前进到下一个顶层条目边界（保证至少前进一个记号）
    fn recover_item(&mut self) {
        let start = self.pos;
        while !self.check(&TokenKind::Eof) {
            if self.pos > start && is_item_boundary(self.peek()) { return; }
            self.advance();
        }
    }

    /// 恐慌模式同步：前进到下一个成员边界，并吃掉一个分隔符避免级联报错
    fn recover_member(&mut self) {
        let start = self.pos;
        while !self.check(&TokenKind::Eof) {
            if self.pos > start && is_member_boundary(self.peek()) { break; }
            self.advance();
        }
        if self.check(&TokenKind::Comma) || self.check(&TokenKind::Semicolon) {
            self.advance();
        }
    }

    /// 执行一步成员解析；失败则记录错误并同步到下一个成员边界
    fn try_member<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Option<T> {
        match f(self) {
            Ok(v) => Some(v),
            Err(e) => { self.errors.push(e); self.recover_member(); None }
        }
    }

    fn parse_module(&mut self) -> Result<Module> {
        let mut name = String::from("unnamed");
        let mut version: u32 = 1;

        while self.check(&TokenKind::At) {
            match self.parse_annotation() {
                Ok(ann) => match ann.name.as_str() {
                    "module" => if let Some(n) = ann.ident_arg(0) { name = n.to_string(); },
                    "version" => if let Some(v) = ann.int_arg(0) { version = v as u32; },
                    _ => {}
                },
                Err(e) => { self.errors.push(e); break; }
            }
        }

        // 顶层条目：错误恢复，一次收集全部错误
        let mut items = Vec::new();
        while !self.check(&TokenKind::Eof) {
            match self.parse_item() {
                Ok(it) => items.push(it),
                Err(e) => { self.errors.push(e); self.recover_item(); }
            }
        }
        Ok(Module { name, version, items })
    }

    fn parse_item(&mut self) -> Result<Item> {
        let anns = self.parse_annotations()?;
        let opaque = anns.iter().any(|a| a.name == "opaque");

        match self.peek().clone() {
            TokenKind::KwEnum => Ok(Item::Enum(self.parse_enum()?)),
            TokenKind::KwError => Ok(Item::Error(self.parse_error()?)),
            TokenKind::KwStruct => Ok(Item::Struct(self.parse_struct(opaque)?)),
            TokenKind::KwInterface => Ok(Item::Interface(self.parse_interface(opaque)?)),
            TokenKind::KwType => Ok(Item::TypeAlias(self.parse_type_alias()?)),
            TokenKind::KwUse => {
                self.skip_use()?;
                if self.check(&TokenKind::Eof) {
                    Err(CompileError::new("use at end of file"))
                } else {
                    self.parse_item()
                }
            }
            other => {
                let t = self.peek_tok();
                Err(CompileError::new(format!(
                    "expected item, got {:?}", other
                )).at(t.line, t.col))
            }
        }
    }

    fn skip_use(&mut self) -> Result<()> {
        self.advance();
        while !self.check(&TokenKind::Semicolon) && !self.check(&TokenKind::Eof) {
            self.advance();
        }
        if self.check(&TokenKind::Semicolon) { self.advance(); }
        Ok(())
    }

    fn parse_enum(&mut self) -> Result<EnumDef> {
        self.expect(&TokenKind::KwEnum, "'enum'")?;
        let name = self.expect_ident()?;
        let underlying = if self.check(&TokenKind::Colon) {
            self.advance(); self.expect_ident()?
        } else { "u32".into() };
        self.expect(&TokenKind::LBrace, "'{'")?;
        let mut variants = Vec::new();
        // 变体级错误恢复
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let v = match self.try_member(|p| p.parse_variant()) {
                Some(v) => v,
                None => continue,
            };
            variants.push(v);
        }
        self.expect(&TokenKind::RBrace, "'}'")?;
        Ok(EnumDef { name, underlying, variants })
    }

    fn parse_variant(&mut self) -> Result<Variant> {
        let anns = self.parse_annotations()?;
        let vname = self.expect_ident()?;
        self.expect(&TokenKind::Equals, "'='")?;
        let val = self.expect_int()?;
        if self.check(&TokenKind::Comma) { self.advance(); }
        Ok(Variant { name: vname, value: val, annotations: anns })
    }

    fn parse_error(&mut self) -> Result<ErrorDef> {
        self.expect(&TokenKind::KwError, "'error'")?;
        let name = self.expect_ident()?;
        let underlying = if self.check(&TokenKind::Colon) {
            self.advance(); self.expect_ident()?
        } else { "i32".into() };
        self.expect(&TokenKind::LBrace, "'{'")?;
        let mut variants = Vec::new();
        // 变体级错误恢复
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let v = match self.try_member(|p| p.parse_variant()) {
                Some(v) => v,
                None => continue,
            };
            variants.push(v);
        }
        self.expect(&TokenKind::RBrace, "'}'")?;
        Ok(ErrorDef { name, underlying, variants })
    }

    fn parse_struct(&mut self, opaque: bool) -> Result<StructDef> {
        self.expect(&TokenKind::KwStruct, "'struct'")?;
        let name = self.expect_ident()?;
        // 可选泛型参数：struct Wrapper<T, U>
        let type_params = self.parse_type_params()?;
        self.expect(&TokenKind::LBrace, "'{'")?;
        let mut fields = Vec::new();
        // 字段级错误恢复：单个字段出错不影响其余字段
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let f = match self.try_member(|p| p.parse_field()) {
                Some(f) => f,
                None => continue,
            };
            fields.push(f);
        }
        self.expect(&TokenKind::RBrace, "'}'")?;
        Ok(StructDef { name, opaque, type_params, fields })
    }

    /// 解析可选的泛型参数列表 `<T, U>`（无则返回空）
    fn parse_type_params(&mut self) -> Result<Vec<String>> {
        if !self.check(&TokenKind::Lt) { return Ok(Vec::new()); }
        self.advance();
        let mut ps = Vec::new();
        loop {
            ps.push(self.expect_ident()?);
            if self.check(&TokenKind::Comma) { self.advance(); } else { break; }
        }
        self.expect(&TokenKind::Gt, "'>'")?;
        Ok(ps)
    }

    fn parse_field(&mut self) -> Result<Field> {
        let anns = self.parse_annotations()?;
        let fname = self.expect_ident()?;
        self.expect(&TokenKind::Colon, "':'")?;
        let ty = self.parse_type()?;
        if self.check(&TokenKind::Comma) { self.advance(); }
        Ok(Field { name: fname, ty, annotations: anns })
    }

    fn parse_interface(&mut self, opaque: bool) -> Result<InterfaceDef> {
        self.expect(&TokenKind::KwInterface, "'interface'")?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace, "'{'")?;
        let mut methods = Vec::new();
        // 方法级错误恢复：单个方法出错不影响其余方法
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let m = match self.try_member(|p| p.parse_method()) {
                Some(m) => m,
                None => continue,
            };
            methods.push(m);
        }
        self.expect(&TokenKind::RBrace, "'}'")?;
        Ok(InterfaceDef { name, opaque, methods })
    }

    fn parse_method(&mut self) -> Result<Method> {
            let anns = self.parse_annotations()?;
            let mname = self.expect_ident()?;
            self.expect(&TokenKind::LParen, "'('")?;

            // self 参数支持 @owned self / @borrowed self
            let mut self_ownership = None;
            let mut params = Vec::new();

            // 检测 self 参数：`@owned self` / `@borrowed self`
            // 词法上 `@` 是独立的 At 记号，需先吃掉可选的 '@'
            if self.check(&TokenKind::At) { self.advance(); }
            if self.check(&TokenKind::KwOwned) || self.check(&TokenKind::KwBorrowed) {
                let own_kw = self.peek().clone();
                // 看是否是 "owned self" 或 "borrowed self"
                let next_is_self = {
                    let p = self.pos + 1;
                    matches!(self.tokens.get(p).map(|t| &t.kind), Some(TokenKind::Ident(s)) if s == "self")
                };
                if next_is_self {
                    self.advance();
                    self.advance(); // 吃掉 self
                    self_ownership = Some(if matches!(own_kw, TokenKind::KwOwned) {
                        Ownership::Owned
                    } else {
                        Ownership::Borrowed
                    });
                    if self.check(&TokenKind::Comma) { self.advance(); }
                }
            }

            while !self.check(&TokenKind::RParen) {
                // 参数所有权前缀：可选的 '@' 后接 owned / borrowed
                let mut pown = Ownership::Value;
                if self.check(&TokenKind::At) { self.advance(); }
                if self.check(&TokenKind::KwOwned) { self.advance(); pown = Ownership::Owned; }
                else if self.check(&TokenKind::KwBorrowed) { self.advance(); pown = Ownership::Borrowed; }

                let pname = self.expect_ident()?;
                self.expect(&TokenKind::Colon, "':'")?;
                let pty = self.parse_type()?;
                params.push(Param { name: pname, ty: pty, ownership: pown });
                if self.check(&TokenKind::Comma) { self.advance(); }
            }
            self.expect(&TokenKind::RParen, "')'")?;
            self.expect(&TokenKind::Arrow, "'->'")?;
            let ret = self.parse_type()?;
            self.expect(&TokenKind::Semicolon, "';'")?;
            Ok(Method {
                name: mname, self_ownership, params, ret, annotations: anns,
            })
    }

    fn parse_type_alias(&mut self) -> Result<TypeAliasDef> {
        self.expect(&TokenKind::KwType, "'type'")?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Equals, "'='")?;
        let target = self.parse_type()?;
        self.expect(&TokenKind::Semicolon, "';'")?;
        Ok(TypeAliasDef { name, target })
    }

    fn parse_type(&mut self) -> Result<TypeRef> {
        // 所有权前缀：'@' 是独立记号，先吃掉可选的 '@'，再匹配 owned / borrowed
        // （使 @owned / @borrowed 在结构体字段与 result<...> 中同样可用）
        if self.check(&TokenKind::At) { self.advance(); }
        if self.check(&TokenKind::KwOwned) {
            self.advance();
            let inner = self.parse_type()?;
            return Ok(TypeRef::Owned(Box::new(inner)));
        }
        if self.check(&TokenKind::KwBorrowed) {
            self.advance();
            let inner = self.parse_type()?;
            return Ok(TypeRef::Borrowed(Box::new(inner)));
        }

        let name = self.expect_ident()?;

        match name.as_str() {
            "Vec" => {
                self.expect(&TokenKind::Lt, "'<'")?;
                let inner = self.parse_type()?;
                self.expect(&TokenKind::Gt, "'>'")?;
                Ok(TypeRef::Vec(Box::new(inner)))
            }
            "Option" => {
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
            "Map" | "Dict" | "map" | "dict" => {
                self.expect(&TokenKind::Lt, "'<'")?;
                let k = self.parse_type()?;
                self.expect(&TokenKind::Comma, "','")?;
                let v = self.parse_type()?;
                self.expect(&TokenKind::Gt, "'>'")?;
                Ok(TypeRef::Map(Box::new(k), Box::new(v)))
            }
            "Set" | "set" => {
                self.expect(&TokenKind::Lt, "'<'")?;
                let t = self.parse_type()?;
                self.expect(&TokenKind::Gt, "'>'")?;
                Ok(TypeRef::Set(Box::new(t)))
            }
            "BorrowedStr" => Ok(TypeRef::BorrowedStr),
            "OwnedString" | "String" => Ok(TypeRef::OwnedString),
            "u8" | "u16" | "u32" | "u64"
            | "i8" | "i16" | "i32" | "i64"
            | "bool" | "unit" => Ok(TypeRef::Primitive(name)),
            _ => {
                // 用户泛型类型应用：Name<A, B>（单态化阶段会改写为具体 Named）
                if self.check(&TokenKind::Lt) {
                    self.advance();
                    let mut args = Vec::new();
                    loop {
                        args.push(self.parse_type()?);
                        if self.check(&TokenKind::Comma) { self.advance(); } else { break; }
                    }
                    self.expect(&TokenKind::Gt, "'>'")?;
                    return Ok(TypeRef::App { name, args });
                }
                Ok(TypeRef::Named(name))
            }
        }
    }

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
                let first = match self.peek().clone() {
                    TokenKind::Ident(s) => { self.advance(); AnnotationArg::Ident(s) }
                    TokenKind::KwAdded => { self.advance(); AnnotationArg::Ident("added".into()) }
                    TokenKind::KwRemoved => { self.advance(); AnnotationArg::Ident("removed".into()) }
                    TokenKind::Int(n) => { self.advance(); AnnotationArg::Int(n) }
                    TokenKind::Str(s) => { self.advance(); AnnotationArg::Str(s) }
                    other => {
                        let t = self.peek_tok();
                        return Err(CompileError::new(format!(
                            "unexpected annotation arg: {:?}", other
                        )).at(t.line, t.col));
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
                        other => {
                            let t = self.peek_tok();
                            return Err(CompileError::new(format!(
                                "unexpected annotation value: {:?}", other
                            )).at(t.line, t.col));
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