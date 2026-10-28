use std::collections::{BTreeMap, BTreeSet};
use crate::ast::*;
use crate::semantic::layout::{self, Layout};

/// BridgeIR 版本：IR 结构变更时递增（可演进 / 兼容性检查）
pub const IR_VERSION: u32 = 1;
/// 指纹算法版本：指纹算法变更时递增（Anchor 自校验）
pub const FINGERPRINT_ALGO_VERSION: u32 = 1;

/// 中间表示：IDL -> 布局/偏移/不透明集合。
/// 所有映射使用有序容器，保证迭代顺序确定 => 生成结果幂等、可 diff。
pub struct BridgeIr {
    pub ir_version: u32,
    pub module: Module,
    /// 共享类型的布局
    pub type_layouts: BTreeMap<String, Layout>,
    /// 共享结构体的字段偏移（有序）
    pub field_offsets: BTreeMap<String, Vec<(String, Layout, usize)>>,
    /// 枚举/错误底层大小
    pub enum_sizes: BTreeMap<String, Layout>,
    /// 不透明类型集合
    pub opaque_types: BTreeSet<String>,
    /// 反序列化得到的规范描述符（用于离线 IR 复算同一指纹）
    pub descriptor_override: Option<String>,
}

impl BridgeIr {
    pub fn build(module: &Module) -> Self {
        let mut ir = Self {
            ir_version: IR_VERSION,
            module: module.clone(),
            type_layouts: BTreeMap::new(),
            field_offsets: BTreeMap::new(),
            enum_sizes: BTreeMap::new(),
            opaque_types: BTreeSet::new(),
            descriptor_override: None,
        };

        // 第一遍：识别 opaque
        for item in &module.items {
            match item {
                Item::Struct(s) if s.opaque => { ir.opaque_types.insert(s.name.clone()); }
                Item::Interface(i) => {
                    // 接口天然不透明
                    ir.opaque_types.insert(i.name.clone());
                }
                _ => {}
            }
        }

        // 第二遍：计算共享类型布局
        for item in &module.items {
            match item {
                Item::Struct(s) if !s.opaque => {
                    let l = layout::compute_struct_layout(module, s);
                    ir.type_layouts.insert(s.name.clone(), l);

                    let mut field_info = Vec::new();
                    let mut offset = 0usize;
                    for f in &s.fields {
                        let fl = layout::layout_of(module, &f.ty);
                        offset = fl.align_up(offset);
                        field_info.push((f.name.clone(), fl, offset));
                        offset += fl.size;
                    }
                    ir.field_offsets.insert(s.name.clone(), field_info);
                }
                Item::Enum(e) => {
                    ir.enum_sizes.insert(e.name.clone(),
                                         layout::size_of_primitive(&e.underlying));
                }
                Item::Error(e) => {
                    ir.enum_sizes.insert(e.name.clone(),
                                         layout::size_of_primitive(&e.underlying));
                }
                _ => {}
            }
        }

        ir
    }

    pub fn is_opaque(&self, name: &str) -> bool {
        self.opaque_types.contains(name)
    }

    pub fn layout_of(&self, name: &str) -> Option<Layout> {
        self.type_layouts.get(name).copied()
    }

    pub fn enum_layout(&self, name: &str) -> Option<Layout> {
        self.enum_sizes.get(name).copied()
    }

    pub fn field_offsets(&self, name: &str) -> Option<&[(String, Layout, usize)]> {
        self.field_offsets.get(name).map(|v| v.as_slice())
    }

    /// 规范描述符：布局的唯一文本表示（排序后拼接），用于指纹与 diff
    pub fn canonical_descriptor(&self) -> String {
        if let Some(d) = &self.descriptor_override { return d.clone(); }
        let mut d = String::new();
        d.push_str(&format!("ir={};algo={};module={};ver={}\n",
            self.ir_version, FINGERPRINT_ALGO_VERSION, self.module.name, self.module.version));
        for (n, l) in &self.type_layouts {
            d.push_str(&format!("S {} size={} align={}\n", n, l.size, l.align));
        }
        for (n, fields) in &self.field_offsets {
            for (fn_, fl, off) in fields {
                d.push_str(&format!("F {}.{} off={} size={} align={}\n",
                    n, fn_, off, fl.size, fl.align));
            }
        }
        for (n, l) in &self.enum_sizes {
            d.push_str(&format!("E {} size={} align={}\n", n, l.size, l.align));
        }
        for n in &self.opaque_types {
            d.push_str(&format!("O {}\n", n));
        }
        // 接口方法签名（影响 VTable ABI）
        for item in &self.module.items {
            if let Item::Interface(i) = item {
                for m in &i.methods {
                    d.push_str(&format!("M {}.{} ({}) -> {}\n",
                        i.name, m.name,
                        m.params.iter().map(|p| ty_name(&p.ty)).collect::<Vec<_>>().join(","),
                        ty_name(&m.ret)));
                }
            }
        }
        d
    }

    /// FNV-1a 32 位指纹（确定性，跨运行稳定）
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for b in self.canonical_descriptor().as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x01000193);
        }
        h
    }

    /// 序列化为 JSON（确定性：键有序）
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        s.push_str("{\n");
        s.push_str(&format!("  \"ir_version\": {},\n", self.ir_version));
        s.push_str(&format!("  \"fingerprint_algo_version\": {},\n", FINGERPRINT_ALGO_VERSION));
        s.push_str(&format!("  \"fingerprint\": {},\n", self.fingerprint()));
        s.push_str(&format!("  \"module\": {{ \"name\": {}, \"version\": {} }},\n",
            jstr(&self.module.name), self.module.version));

        // 类型布局
        s.push_str("  \"type_layouts\": {\n");
        for (i, (n, l)) in self.type_layouts.iter().enumerate() {
            s.push_str(&format!("    {}: {{ \"size\": {}, \"align\": {} }}{}\n",
                jstr(n), l.size, l.align,
                if i + 1 == self.type_layouts.len() { "" } else { "," }));
        }
        s.push_str("  },\n");

        // 字段偏移
        s.push_str("  \"field_offsets\": {\n");
        for (i, (n, fields)) in self.field_offsets.iter().enumerate() {
            s.push_str(&format!("    {}: [", jstr(n)));
            for (j, (fn_, fl, off)) in fields.iter().enumerate() {
                s.push_str(&format!("{}{{ \"name\": {}, \"offset\": {}, \"size\": {}, \"align\": {} }}",
                    if j == 0 { "" } else { ", " }, jstr(fn_), off, fl.size, fl.align));
            }
            s.push_str(&format!("]{}\n", if i + 1 == self.field_offsets.len() { "" } else { "," }));
        }
        s.push_str("  },\n");

        // 枚举/错误
        s.push_str("  \"enum_sizes\": {\n");
        for (i, (n, l)) in self.enum_sizes.iter().enumerate() {
            s.push_str(&format!("    {}: {{ \"size\": {}, \"align\": {} }}{}\n",
                jstr(n), l.size, l.align,
                if i + 1 == self.enum_sizes.len() { "" } else { "," }));
        }
        s.push_str("  },\n");

        // 不透明集合
        s.push_str("  \"opaque_types\": [");
        for (i, n) in self.opaque_types.iter().enumerate() {
            s.push_str(&format!("{}{}", if i == 0 { "" } else { ", " }, jstr(n)));
        }
        s.push_str("]\n");
        s.push_str("}\n");
        s
    }

    /// 编码为确定性二进制（实现见 `encode_bin`）
    pub fn to_bin(&self) -> Vec<u8> { encode_bin(self) }

    /// 由二进制解码（实现见 `decode_bin`）
    pub fn from_bin(bytes: &[u8]) -> std::result::Result<Self, String> { decode_bin(bytes) }

    /// 与另一份 IR 对比，输出人类可读差异；前缀 [break] 表示破坏性变更
    pub fn diff(&self, other: &BridgeIr) -> Vec<String> {
        let mut out = Vec::new();
        if self.ir_version != other.ir_version {
            out.push(format!("[break] ir_version {} -> {}", self.ir_version, other.ir_version));
        }
        if self.module.version != other.module.version {
            out.push(format!("[compat] module version {} -> {}",
                self.module.version, other.module.version));
        }
        for (n, l) in &self.type_layouts {
            match other.type_layouts.get(n) {
                None => out.push(format!("[break] type '{}' removed", n)),
                Some(o) if o.size != l.size || o.align != l.align =>
                    out.push(format!("[break] type '{}' layout {} -> {}",
                        n, layout_str(*l), layout_str(*o))),
                _ => {}
            }
        }
        for n in other.type_layouts.keys() {
            if !self.type_layouts.contains_key(n) {
                out.push(format!("[compat] type '{}' added", n));
            }
        }
        for (n, fields) in &self.field_offsets {
            if let Some(of) = other.field_offsets.get(n) {
                for (fn_, fl, off) in fields {
                    if let Some((_, ofl, ooff)) = of.iter().find(|(x, _, _)| x == fn_) {
                        if off != ooff {
                            out.push(format!("[break] field '{}.{}' offset {} -> {}", n, fn_, off, ooff));
                        }
                        // 字段自身大小/对齐变化同样是 ABI 破坏
                        if fl.size != ofl.size || fl.align != ofl.align {
                            out.push(format!("[break] field '{}.{}' layout {} -> {}",
                                n, fn_, layout_str(*fl), layout_str(*ofl)));
                        }
                    } else {
                        out.push(format!("[break] field '{}.{}' removed", n, fn_));
                    }
                }
            }
        }
        for (n, l) in &self.enum_sizes {
            match other.enum_sizes.get(n) {
                None => out.push(format!("[break] enum '{}' removed", n)),
                Some(o) if o.size != l.size =>
                    out.push(format!("[break] enum '{}' size {} -> {}", n, l.size, o.size)),
                _ => {}
            }
        }
        if self.fingerprint() == other.fingerprint() && out.is_empty() {
            out.push("[none] no ABI change".to_string());
        }
        out
    }
}

// ===================== 二进制序列化（无三方依赖） =====================
//
// 目标：可被存储、可被版本化管理、可离线反序列化做真实 diff。
// 格式（全部大端，确定性）：
//   "XSOIR" | u8 ir_version | u32 module_version | str name
//   | u32 n_layouts   { str name, u64 size, u64 align }
//   | u32 n_groups    { str struct, u32 n_fields { str field, u64 off, u64 size, u64 align } }
//   | u32 n_enums     { str name, u64 size, u64 align }
//   | u32 n_opaque    { str name }
//   | u32 fingerprint | str descriptor

const BIN_MAGIC: &[u8; 5] = b"XSOIR";

pub fn encode_bin(ir: &BridgeIr) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(BIN_MAGIC);
    b.push(ir.ir_version as u8);
    put_u32(&mut b, ir.module.version);
    put_str(&mut b, &ir.module.name);

    put_u32(&mut b, ir.type_layouts.len() as u32);
    for (n, l) in &ir.type_layouts {
        put_str(&mut b, n);
        put_u64(&mut b, l.size as u64);
        put_u64(&mut b, l.align as u64);
    }

    put_u32(&mut b, ir.field_offsets.len() as u32);
    for (n, fields) in &ir.field_offsets {
        put_str(&mut b, n);
        put_u32(&mut b, fields.len() as u32);
        for (fn_, fl, off) in fields {
            put_str(&mut b, fn_);
            put_u64(&mut b, *off as u64);
            put_u64(&mut b, fl.size as u64);
            put_u64(&mut b, fl.align as u64);
        }
    }

    put_u32(&mut b, ir.enum_sizes.len() as u32);
    for (n, l) in &ir.enum_sizes {
        put_str(&mut b, n);
        put_u64(&mut b, l.size as u64);
        put_u64(&mut b, l.align as u64);
    }

    put_u32(&mut b, ir.opaque_types.len() as u32);
    for n in &ir.opaque_types { put_str(&mut b, n); }

    put_u32(&mut b, ir.fingerprint());
    put_str(&mut b, &ir.canonical_descriptor());
    b
}

/// 反序列化；失败返回原因字符串
pub fn decode_bin(bytes: &[u8]) -> std::result::Result<BridgeIr, String> {
    let mut p = 0usize;
    if bytes.len() < BIN_MAGIC.len() || &bytes[..BIN_MAGIC.len()] != BIN_MAGIC {
        return Err("bad magic".into());
    }
    p += BIN_MAGIC.len();
    let ir_version = bytes[p] as u32; p += 1;
    let (module_version, np) = get_u32(bytes, p)?; p = np;
    let (name, np) = get_str(bytes, p)?; p = np;

    let mut ir = BridgeIr {
        ir_version,
        module: crate::ast::Module {
            name, version: module_version, items: Vec::new(), imports: Vec::new(),
        },
        type_layouts: BTreeMap::new(),
        field_offsets: BTreeMap::new(),
        enum_sizes: BTreeMap::new(),
        opaque_types: BTreeSet::new(),
        descriptor_override: None,
    };

    let (n, np) = get_u32(bytes, p)?; p = np;
    for _ in 0..n {
        let (k, np) = get_str(bytes, p)?; p = np;
        let (size, np) = get_u64(bytes, p)?; p = np;
        let (align, np) = get_u64(bytes, p)?; p = np;
        ir.type_layouts.insert(k, Layout { size: size as usize, align: align as usize });
    }

    let (g, np) = get_u32(bytes, p)?; p = np;
    for _ in 0..g {
        let (k, np) = get_str(bytes, p)?; p = np;
        let (nf, np) = get_u32(bytes, p)?; p = np;
        let mut v = Vec::new();
        for _ in 0..nf {
            let (fn_, np) = get_str(bytes, p)?; p = np;
            let (off, np) = get_u64(bytes, p)?; p = np;
            let (size, np) = get_u64(bytes, p)?; p = np;
            let (align, np) = get_u64(bytes, p)?; p = np;
            v.push((fn_, Layout { size: size as usize, align: align as usize }, off as usize));
        }
        ir.field_offsets.insert(k, v);
    }

    let (e, np) = get_u32(bytes, p)?; p = np;
    for _ in 0..e {
        let (k, np) = get_str(bytes, p)?; p = np;
        let (size, np) = get_u64(bytes, p)?; p = np;
        let (align, np) = get_u64(bytes, p)?; p = np;
        ir.enum_sizes.insert(k, Layout { size: size as usize, align: align as usize });
    }

    let (o, np) = get_u32(bytes, p)?; p = np;
    for _ in 0..o {
        let (k, np) = get_str(bytes, p)?; p = np;
        ir.opaque_types.insert(k);
    }

    let (_fp, np) = get_u32(bytes, p)?; p = np;
    let (desc, _np) = get_str(bytes, p)?;
    ir.descriptor_override = Some(desc);
    Ok(ir)
}

fn put_u32(b: &mut Vec<u8>, v: u32) { b.extend_from_slice(&v.to_be_bytes()); }
fn put_u64(b: &mut Vec<u8>, v: u64) { b.extend_from_slice(&v.to_be_bytes()); }
fn put_str(b: &mut Vec<u8>, s: &str) {
    put_u32(b, s.len() as u32);
    b.extend_from_slice(s.as_bytes());
}
fn get_u32(b: &[u8], p: usize) -> std::result::Result<(u32, usize), String> {
    if p + 4 > b.len() { return Err("truncated u32".into()); }
    let mut a = [0u8; 4]; a.copy_from_slice(&b[p..p + 4]);
    Ok((u32::from_be_bytes(a), p + 4))
}
fn get_u64(b: &[u8], p: usize) -> std::result::Result<(u64, usize), String> {
    if p + 8 > b.len() { return Err("truncated u64".into()); }
    let mut a = [0u8; 8]; a.copy_from_slice(&b[p..p + 8]);
    Ok((u64::from_be_bytes(a), p + 8))
}
fn get_str(b: &[u8], p: usize) -> std::result::Result<(String, usize), String> {
    let (len, np) = get_u32(b, p)?;
    let end = np.checked_add(len as usize).ok_or("bad str len")?;
    if end > b.len() { return Err("truncated str".into()); }
    Ok((String::from_utf8_lossy(&b[np..end]).into_owned(), end))
}

fn layout_str(l: Layout) -> String {
    format!("(size={},align={})", l.size, l.align)
}

fn jstr(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 类型的规范名（用于 IR 描述与 diff）
fn ty_name(t: &TypeRef) -> String {
    match t {
        TypeRef::Primitive(p) => p.clone(),
        TypeRef::Named(n) => n.clone(),
        TypeRef::App { name, args } => crate::semantic::mono::mangle(name, args),
        TypeRef::BorrowedStr => "BorrowedStr".into(),
        TypeRef::OwnedString => "OwnedString".into(),
        TypeRef::Vec(i) => format!("Vec<{}>", ty_name(i)),
        TypeRef::Set(i) => format!("Set<{}>", ty_name(i)),
        TypeRef::Map(k, v) => format!("Map<{},{}>", ty_name(k), ty_name(v)),
        TypeRef::Option(i) => format!("Option<{}>", ty_name(i)),
        TypeRef::Owned(i) => format!("@owned {}", ty_name(i)),
        TypeRef::Borrowed(i) => format!("@borrowed {}", ty_name(i)),
        TypeRef::Result(o, e) => format!("result<{},{}>", ty_name(o), ty_name(e)),
    }
}
