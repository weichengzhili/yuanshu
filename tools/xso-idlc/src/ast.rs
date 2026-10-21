#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub version: u32,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Enum(EnumDef),
    Error(ErrorDef),
    Struct(StructDef),
    Interface(InterfaceDef),
    TypeAlias(TypeAliasDef),
}

#[derive(Clone, Debug)]
pub struct EnumDef {
    pub name: String,
    pub underlying: String,
    pub variants: Vec<Variant>,
}

#[derive(Clone, Debug)]
pub struct ErrorDef {
    pub name: String,
    pub underlying: String,
    pub variants: Vec<Variant>,
}

#[derive(Clone, Debug)]
pub struct Variant {
    pub name: String,
    pub value: i64,
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug)]
pub struct StructDef {
    pub name: String,
    pub opaque: bool,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub ty: TypeRef,
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug)]
pub struct InterfaceDef {
    pub name: String,
    pub opaque: bool,
    pub methods: Vec<Method>,
}

#[derive(Clone, Debug)]
pub struct Method {
    pub name: String,
    pub self_ownership: Option<Ownership>,
    pub params: Vec<Param>,
    pub ret: TypeRef,
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: TypeRef,
    pub ownership: Ownership,
}

/// 所有权语义（边界控制核心）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ownership {
    /// 按值传递
    Value,
    /// 借用：调用期间有效
    Borrowed,
    /// 转移所有权
    Owned,
}

#[derive(Clone, Debug)]
pub struct TypeAliasDef {
    pub name: String,
    pub target: TypeRef,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeRef {
    Primitive(String),
    BorrowedStr,
    OwnedString,
    Named(String),
    Vec(Box<TypeRef>),
    Option(Box<TypeRef>),
    Result(Box<TypeRef>, Box<TypeRef>),
    /// @owned T
    Owned(Box<TypeRef>),
    /// @borrowed T
    Borrowed(Box<TypeRef>),
}

#[derive(Clone, Debug)]
pub struct Annotation {
    pub name: String,
    pub args: Vec<(Option<String>, AnnotationArg)>,
}

#[derive(Clone, Debug)]
pub enum AnnotationArg {
    Ident(String),
    Int(i64),
    Str(String),
}

impl Annotation {
    pub fn int_arg(&self, idx: usize) -> Option<i64> {
        match self.args.get(idx)?.1 {
            AnnotationArg::Int(n) => Some(n),
            _ => None,
        }
    }
    pub fn ident_arg(&self, idx: usize) -> Option<&str> {
        match &self.args.get(idx)?.1 {
            AnnotationArg::Ident(s) => Some(s.as_str()),
            _ => None,
        }
    }
    pub fn named_int(&self, key: &str) -> Option<i64> {
        for (k, v) in &self.args {
            if k.as_deref() == Some(key) {
                if let AnnotationArg::Int(n) = v { return Some(*n); }
            }
        }
        None
    }
}

impl Module {
    pub fn find_item(&self, name: &str) -> Option<&Item> {
        self.items.iter().find(|it| match it {
            Item::Enum(e) => e.name == name,
            Item::Error(e) => e.name == name,
            Item::Struct(s) => s.name == name,
            Item::Interface(i) => i.name == name,
            Item::TypeAlias(t) => t.name == name,
        })
    }
    pub fn is_opaque(&self, name: &str) -> bool {
        match self.find_item(name) {
            Some(Item::Struct(s)) => s.opaque,
            Some(Item::Interface(i)) => i.opaque,
            _ => false,
        }
    }
}