#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub version: u32,
    pub items: Vec<Item>,
    /// 顶层 `use` 导入声明（多文件依赖校验的输入；单文件编译时不生效）
    pub imports: Vec<Import>,
}

/// 一条导入声明：`use <module>::<Name>;` / `use <module>::{A, B};` / `use <module>;`
#[derive(Clone, Debug)]
pub struct Import {
    /// 模块路径（至少一段）；最后一段为模块名，其余仅作展示
    pub path: Vec<String>,
    /// 导入的符号；空表示整模块导入（该模块全部导出符号可见）
    pub names: Vec<ImportName>,
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Debug)]
pub struct ImportName {
    /// 被导入模块中的符号名
    pub name: String,
    /// `as <别名>`：本地可见名变为别名
    pub alias: Option<String>,
    pub line: usize,
    pub col: usize,
}

impl Import {
    /// 模块名（路径最后一段）
    pub fn module_name(&self) -> String {
        self.path.last().cloned().unwrap_or_default()
    }
    /// 路径的可读形式 `a::b`
    pub fn path_str(&self) -> String { self.path.join("::") }
    /// 是否整模块导入
    pub fn is_glob(&self) -> bool { self.names.is_empty() }
}

impl ImportName {
    /// 导入方模块内的可见名
    pub fn local(&self) -> String {
        self.alias.clone().unwrap_or_else(|| self.name.clone())
    }
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
    /// 泛型参数（如 struct Wrapper<T>）；单态化后会生成具体结构体
    pub type_params: Vec<String>,
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
    /// 用户泛型结构体的类型应用：`Wrapper<u32>`。单态化阶段会改写为 Named(单态名)
    App { name: String, args: Vec<TypeRef> },
    Vec(Box<TypeRef>),
    Option(Box<TypeRef>),
    Result(Box<TypeRef>, Box<TypeRef>),
    /// 关联容器 map<K,V>：边界上收敛为 XsoVec<XsoPair<K,V>>
    Map(Box<TypeRef>, Box<TypeRef>),
    /// 集合 set<T>：边界上收敛为 XsoVec<T>
    Set(Box<TypeRef>),
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

impl Item {
    /// 条目名（与类型引用中的 `Named(name)` 对应）
    pub fn name(&self) -> &str {
        match self {
            Item::Enum(e) => &e.name,
            Item::Error(e) => &e.name,
            Item::Struct(s) => &s.name,
            Item::Interface(i) => &i.name,
            Item::TypeAlias(t) => &t.name,
        }
    }
    /// 返回改名为 `new` 的副本（多文件链接时把导入符号改写为本地可见名）
    pub fn renamed(&self, new: &str) -> Item {
        let n = new.to_string();
        match self {
            Item::Enum(e) => Item::Enum(EnumDef { name: n, ..e.clone() }),
            Item::Error(e) => Item::Error(ErrorDef { name: n, ..e.clone() }),
            Item::Struct(s) => Item::Struct(StructDef { name: n, ..s.clone() }),
            Item::Interface(i) => Item::Interface(InterfaceDef { name: n, ..i.clone() }),
            Item::TypeAlias(t) => Item::TypeAlias(TypeAliasDef { name: n, ..t.clone() }),
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
    /// 本模块定义的全部顶层符号名（有序）
    pub fn defined_names(&self) -> std::collections::BTreeSet<String> {
        self.items.iter().map(|it| it.name().to_string()).collect()
    }
    /// 本模块**引用**到的具名类型集合（不含基元与内建容器）
    pub fn referenced_names(&self) -> std::collections::BTreeSet<String> {
        let mut out = std::collections::BTreeSet::new();
        for it in &self.items { collect_named_refs(it, &mut out); }
        out
    }
    pub fn is_opaque(&self, name: &str) -> bool {
        match self.find_item(name) {
            Some(Item::Struct(s)) => s.opaque,
            Some(Item::Interface(i)) => i.opaque,
            _ => false,
        }
    }
}

/// 收集一个条目引用到的全部具名类型（含泛型应用的名字）
pub fn collect_named_refs(item: &Item, out: &mut std::collections::BTreeSet<String>) {
    match item {
        Item::Enum(_) | Item::Error(_) => {}
        Item::Struct(s) => for f in &s.fields { type_named_refs(&f.ty, out); },
        Item::Interface(i) => for m in &i.methods {
            for p in &m.params { type_named_refs(&p.ty, out); }
            type_named_refs(&m.ret, out);
        },
        Item::TypeAlias(t) => type_named_refs(&t.target, out),
    }
}

/// 收集一个类型引用中的具名类型（`Named` 与泛型应用 `App` 的名字）
pub fn type_named_refs(ty: &TypeRef, out: &mut std::collections::BTreeSet<String>) {
    match ty {
        TypeRef::Named(n) => { out.insert(n.clone()); }
        TypeRef::App { name, args } => {
            out.insert(name.clone());
            for a in args { type_named_refs(a, out); }
        }
        TypeRef::Vec(i) | TypeRef::Option(i) | TypeRef::Owned(i)
        | TypeRef::Borrowed(i) | TypeRef::Set(i) => type_named_refs(i, out),
        TypeRef::Map(k, v) | TypeRef::Result(k, v) => {
            type_named_refs(k, out);
            type_named_refs(v, out);
        }
        TypeRef::Primitive(_) | TypeRef::BorrowedStr | TypeRef::OwnedString => {}
    }
}