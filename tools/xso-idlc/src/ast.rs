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
    pub methods: Vec<Method>,
}

#[derive(Clone, Debug)]
pub struct Method {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: TypeRef,
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: TypeRef,
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
}

#[derive(Clone, Debug)]
pub struct Annotation {
    pub name: String,
    /// (key, value) —— key 为 None 表示位置参数
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
    pub fn named_int(&self, key: &str) -> Option<i64> {
        for (k, v) in &self.args {
            if k.as_deref() == Some(key) {
                if let AnnotationArg::Int(n) = v { return Some(*n); }
            }
        }
        None
    }
    pub fn ident_arg(&self, idx: usize) -> Option<&str> {
        match &self.args.get(idx)?.1 {
            AnnotationArg::Ident(s) => Some(s.as_str()),
            _ => None,
        }
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
}

impl TypeRef {
    /// 转成 `TypeRef::Named` 里的名字（用于语义检查）
    pub fn named(&self) -> Option<&str> {
        match self {
            TypeRef::Named(n) => Some(n),
            _ => None,
        }
    }
}