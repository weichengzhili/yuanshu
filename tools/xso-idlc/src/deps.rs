//! 多文件依赖校验（multi-file dependency validation）。
//!
//! 单文件校验只能保证「一个 IDL 自洽」；本模块把门禁提升到**工程级**：
//!
//! | 能力 | 说明 |
//! |---|---|
//! | 显式依赖声明 | 依赖只能写成 `use <module>::<Symbol>;` / `use <module>::{A,B};` / `use <module>;` |
//! | 模块解析 | 被导入模块必须在输入集合中，或能在 `--include-dir`（含输入所在目录）中发现 |
//! | 符号解析 | 被导入符号必须存在于目标模块的导出中 |
//! | 冲突检查 | 自我导入 / 重复导入 / 与本地定义冲突 / 导入符号互相碰撞 |
//! | 环检测 | 模块级依赖环（无法拓扑排序即无法按序构建） |
//! | 未使用导入 | 默认警告，`--strict` 升级为错误 |
//! | 拓扑顺序 | 输出构建顺序，供构建系统按依赖次序生成 |
//! | 链接 | 被导入符号（含其传递依赖）克隆进导入方，产物自包含 |
//!
//! 设计要点：模块图**无环** ⇒ 依赖闭包展开必然终止；克隆进来的定义与源模块
//! 使用完全相同的布局算法，因此两端布局常量一致（ABI 不变）。

use crate::ast::{self, Item, Module};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 工程内文件总数上限（含从 include 目录发现的依赖）
const MAX_FILES: usize = 512;
/// 依赖闭包展开步数上限（防病态输入导致组合爆炸）
const MAX_LINK_STEPS: usize = 4096;
/// 单个文件的解析错误最多报告多少条
const MAX_REPORTED_PARSE_ERRORS: usize = 16;
/// 源文件字节上限（与 main 保持一致）
const MAX_SRC_BYTES: usize = 8 * 1024 * 1024;

// ===================== 诊断 =====================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self { Severity::Error => "error", Severity::Warning => "warning" }
    }
}

/// 一条跨文件诊断（带文件/行列，便于 `file:line:col:` 定位）
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
    pub hint: Option<String>,
}

impl Diagnostic {
    fn new(sev: Severity, file: &Path, line: usize, col: usize, msg: impl Into<String>) -> Self {
        Self {
            severity: sev,
            file: Some(file.display().to_string()),
            line: Some(line),
            column: Some(col),
            message: msg.into(),
            hint: None,
        }
    }
    fn error(file: &Path, line: usize, col: usize, msg: impl Into<String>) -> Self {
        Diagnostic::new(Severity::Error, file, line, col, msg)
    }
    fn warn(file: &Path, line: usize, col: usize, msg: impl Into<String>) -> Self {
        Diagnostic::new(Severity::Warning, file, line, col, msg)
    }
    fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// `file:line:col: error: message` + 可选 hint
    pub fn render(&self) -> String {
        let mut s = String::new();
        if let Some(f) = &self.file { s.push_str(&format!("{}:", f)); }
        if let Some(l) = self.line {
            s.push_str(&format!("{}:", l));
            if let Some(c) = self.column { s.push_str(&format!("{}:", c)); }
        }
        s.push_str(&format!(" {}: {}", self.severity.as_str(), self.message));
        if let Some(h) = &self.hint { s.push_str(&format!("\n  hint: {}", h)); }
        s
    }

}

// ===================== 数据模型 =====================

/// 一个已加载的 IDL 文件
struct ModFile {
    pub path: PathBuf,
    pub module: Module,
    /// 是否由 `--input` 显式给出（false = 作为依赖被发现）
    pub is_input: bool,
    /// 源文本哈希（用于增量缓存的跨文件失效）
    pub src_hash: u64,
}

/// 解析成功的导入（target 为 `files` 下标）
struct ResolvedImport {
    target: usize,
    /// 导入方模块内的可见名（整模块导入为 None）
    local: Option<String>,
    /// 源模块中的符号名（整模块导入为 None）
    symbol: Option<String>,
    line: usize,
    col: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// 警告升级为错误（CI 严格模式）
    pub strict: bool,
    /// 模块搜索目录（各输入所在目录会自动加入）
    pub include_dirs: Vec<PathBuf>,
}

/// 依赖图中的一个模块节点
pub struct ModuleInfo {
    pub name: String,
    pub version: u32,
    pub file: PathBuf,
    pub is_input: bool,
    /// 导出的顶层符号（有序）
    pub exports: Vec<String>,
    /// 直接依赖的模块（有序、去重）
    pub deps: Vec<String>,
    /// 导入的符号本地名（有序）
    pub imports: Vec<String>,
}

/// 多文件依赖校验结果
pub struct Report {
    /// 参与校验的模块（按模块名排序）
    pub modules: Vec<ModuleInfo>,
    /// 依赖边 `(from, to)`（有序、去重）
    pub edges: Vec<(String, String)>,
    /// 拓扑构建顺序（存在环时为无环部分的偏序）
    pub order: Vec<String>,
    /// 检测到的依赖环（每个为一条环路径）
    pub cycles: Vec<Vec<String>>,
    pub diagnostics: Vec<Diagnostic>,
    /// 每个**输入**文件需要内联的外部定义（路径 -> 追加到 items 的条目）
    pub linked: BTreeMap<PathBuf, Vec<Item>>,
    /// 每个输入文件的依赖签名（依赖内容变化即失效增量缓存）
    pub signatures: BTreeMap<PathBuf, u64>,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Error).count()
    }
    pub fn warnings(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Warning).count()
    }
    /// 是否存在必须中止构建的问题（`strict` 时警告也算）
    pub fn has_errors(&self) -> bool { self.errors() > 0 }
    pub fn is_fatal(&self, strict: bool) -> bool {
        self.errors() > 0 || (strict && self.warnings() > 0)
    }

    /// 确定性 JSON（键有序、数组有序），供 CI 消费
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        s.push_str("{\n");
        s.push_str("  \"modules\": [\n");
        for (i, m) in self.modules.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"name\": \"{}\",\n", json_escape(&m.name)));
            s.push_str(&format!("      \"version\": {},\n", m.version));
            s.push_str(&format!("      \"file\": \"{}\",\n", json_escape(&m.file.display().to_string())));
            s.push_str(&format!("      \"input\": {},\n", m.is_input));
            s.push_str(&format!("      \"exports\": [{}],\n", json_strings(&m.exports)));
            s.push_str(&format!("      \"deps\": [{}],\n", json_strings(&m.deps)));
            s.push_str(&format!("      \"imports\": [{}]\n", json_strings(&m.imports)));
            s.push_str(if i + 1 == self.modules.len() { "    }\n" } else { "    },\n" });
        }
        s.push_str("  ],\n");
        s.push_str("  \"edges\": [\n");
        for (i, (a, b)) in self.edges.iter().enumerate() {
            s.push_str(&format!("    {{\"from\": \"{}\", \"to\": \"{}\"}}{}\n",
                json_escape(a), json_escape(b),
                if i + 1 == self.edges.len() { "" } else { "," }));
        }
        s.push_str("  ],\n");
        s.push_str(&format!("  \"order\": [{}],\n", json_strings(&self.order)));
        s.push_str("  \"cycles\": [\n");
        for (i, c) in self.cycles.iter().enumerate() {
            s.push_str(&format!("    [{}]{}\n", json_strings(c),
                if i + 1 == self.cycles.len() { "" } else { "," }));
        }
        s.push_str("  ],\n");
        s.push_str(&format!("  \"errors\": {},\n", self.errors()));
        s.push_str(&format!("  \"warnings\": {},\n", self.warnings()));
        s.push_str("  \"diagnostics\": [\n");
        for (i, d) in self.diagnostics.iter().enumerate() {
            s.push_str(&format!(
                "    {{\"severity\": \"{}\", \"file\": \"{}\", \"line\": {}, \"column\": {}, \
                 \"message\": \"{}\", \"hint\": \"{}\"}}{}\n",
                d.severity.as_str(),
                json_escape(d.file.as_deref().unwrap_or("")),
                d.line.map(|v| v as i64).unwrap_or(-1),
                d.column.map(|v| v as i64).unwrap_or(-1),
                json_escape(&d.message),
                json_escape(d.hint.as_deref().unwrap_or("")),
                if i + 1 == self.diagnostics.len() { "" } else { "," }));
        }
        s.push_str("  ]\n");
        s.push_str("}\n");
        s
    }
}

fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

fn json_strings(xs: &[String]) -> String {
    xs.iter().map(|x| format!("\"{}\"", json_escape(x))).collect::<Vec<_>>().join(", ")
}

// ===================== 加载 =====================

fn load_file(path: &Path, is_input: bool, diags: &mut Vec<Diagnostic>) -> Option<ModFile> {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            diags.push(Diagnostic::error(path, 1, 1, format!("cannot read {}: {}", path.display(), e)));
            return None;
        }
    };
    if src.len() > MAX_SRC_BYTES {
        diags.push(Diagnostic::error(path, 1, 1, format!(
            "input too large: {} bytes (limit {})", src.len(), MAX_SRC_BYTES)));
        return None;
    }
    let (m, errs) = crate::parser::parse_with_recovery(&src);
    if !errs.is_empty() {
        for e in errs.iter().take(MAX_REPORTED_PARSE_ERRORS) {
            let d = Diagnostic::error(path,
                e.line.unwrap_or(1), e.column.unwrap_or(1), e.message.clone());
            diags.push(match &e.hint { Some(h) => d.with_hint(h.clone()), None => d });
        }
        if errs.len() > MAX_REPORTED_PARSE_ERRORS {
            diags.push(Diagnostic::error(path, 1, 1,
                format!("... and {} more parse error(s)", errs.len() - MAX_REPORTED_PARSE_ERRORS)));
        }
        // 语法有错时 AST 不完整，继续做依赖分析只会产生误报
        return None;
    }
    let module = m?;
    if module.name == "unnamed" {
        diags.push(Diagnostic::warn(path, 1, 1,
            "module has no @module(<name>); cross-file imports cannot reference it")
            .with_hint("add `@module(<name>)` at the top of the file"));
    }
    Some(ModFile { path: path.to_path_buf(), module, is_input, src_hash: crate::fnv1a(&src) })
}

/// 从搜索目录发现模块文件 `<dir>/<module>.xso`
fn discover(
    files: &mut Vec<ModFile>,
    by_module: &mut BTreeMap<String, usize>,
    dirs: &[PathBuf],
    name: &str,
    diags: &mut Vec<Diagnostic>,
) -> Option<usize> {
    for d in dirs {
        let cand = d.join(format!("{}.xso", name));
        if !cand.is_file() { continue; }
        if let Some(i) = files.iter().position(|f| f.path == cand) { return Some(i); }
        if files.len() >= MAX_FILES {
            diags.push(Diagnostic::error(&cand, 1, 1,
                format!("too many files in project (limit {})", MAX_FILES)));
            return None;
        }
        let mut tmp: Vec<Diagnostic> = Vec::new();
        match load_file(&cand, false, &mut tmp) {
            Some(f) => {
                diags.extend(tmp);
                if f.module.name != name {
                    diags.push(Diagnostic::warn(&cand, 1, 1, format!(
                        "file declares module '{}', but '{}' was requested", f.module.name, name)));
                    continue;
                }
                let idx = files.len();
                let mname = f.module.name.clone();
                files.push(f);
                by_module.insert(mname, idx);
                return Some(idx);
            }
            None => { diags.extend(tmp); }
        }
    }
    None
}

// ===================== 主流程 =====================

/// 对整个工程做依赖校验；不返回 `Result`，全部问题收集在 `Report::diagnostics`。
pub fn analyze(inputs: &[PathBuf], opts: &Options) -> Report {
    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut files: Vec<ModFile> = Vec::new();

    for p in inputs {
        if let Some(f) = load_file(p, true, &mut diags) { files.push(f); }
    }

    // 模块名索引：重复即报错（两个文件声明同一个 @module）
    let mut by_module: BTreeMap<String, usize> = BTreeMap::new();
    for (i, f) in files.iter().enumerate() {
        if let Some(prev) = by_module.get(&f.module.name).copied() {
            diags.push(Diagnostic::error(&f.path, 1, 1, format!(
                "duplicate module name '{}' (also defined in {})",
                f.module.name, files[prev].path.display()))
                .with_hint("module names must be unique across a project"));
            continue;
        }
        by_module.insert(f.module.name.clone(), i);
    }

    // 搜索目录：显式 --include-dir + 各输入所在目录
    let mut dirs: Vec<PathBuf> = opts.include_dirs.clone();
    for p in inputs {
        if let Some(d) = p.parent() {
            if !d.as_os_str().is_empty() && !dirs.iter().any(|x| x == d) {
                dirs.push(d.to_path_buf());
            }
        }
    }

    // ===== 导入解析（会顺带把新发现的依赖文件拉进来，故用工作队列）=====
    let mut resolved: BTreeMap<usize, Vec<ResolvedImport>> = BTreeMap::new();
    let mut queue: Vec<usize> = (0..files.len()).collect();
    let mut visited: BTreeSet<usize> = BTreeSet::new();
    while let Some(fi) = queue.pop() {
        if !visited.insert(fi) { continue; }
        let self_name = files[fi].module.name.clone();
        let local_defs = files[fi].module.defined_names();
        let imports = files[fi].module.imports.clone();
        let path = files[fi].path.clone();
        let mut list: Vec<ResolvedImport> = Vec::new();

        if imports.len() > crate::parser::MAX_IMPORTS {
            diags.push(Diagnostic::error(&path, 1, 1, format!(
                "too many 'use' declarations: {} (limit {})",
                imports.len(), crate::parser::MAX_IMPORTS)));
        }

        for imp in &imports {
            let target_name = imp.module_name();
            if target_name.is_empty() {
                diags.push(Diagnostic::error(&path, imp.line, imp.col,
                    "empty module name in 'use' declaration"));
                continue;
            }
            if target_name == self_name {
                diags.push(Diagnostic::error(&path, imp.line, imp.col,
                    format!("module '{}' imports itself", self_name))
                    .with_hint("remove the self-import"));
                continue;
            }
            let ti = match by_module.get(&target_name).copied() {
                Some(i) => i,
                None => match discover(&mut files, &mut by_module, &dirs, &target_name, &mut diags) {
                    Some(i) => { queue.push(i); i }
                    None => {
                        let known: Vec<String> = by_module.keys().cloned().collect();
                        diags.push(Diagnostic::error(&path, imp.line, imp.col,
                            format!("unknown module '{}' imported by '{}'", target_name, self_name))
                            .with_hint(format!(
                                "known modules: {}；或把该模块的 .xso 加入 --input / --include-dir",
                                if known.is_empty() { "(none)".to_string() } else { known.join(", ") })));
                        continue;
                    }
                },
            };

            if imp.is_glob() {
                let exports = files[ti].module.defined_names();
                if exports.is_empty() {
                    diags.push(Diagnostic::warn(&path, imp.line, imp.col,
                        format!("module '{}' exports nothing; 'use {};' imports no symbols",
                            target_name, imp.path_str())));
                }
                list.push(ResolvedImport {
                    target: ti, local: None, symbol: None, line: imp.line, col: imp.col,
                });
                continue;
            }

            for n in &imp.names {
                if files[ti].module.find_item(&n.name).is_none() {
                    let available: Vec<String> = files[ti].module.defined_names().into_iter().collect();
                    diags.push(Diagnostic::error(&path, n.line, n.col, format!(
                        "module '{}' does not export symbol '{}'", target_name, n.name))
                        .with_hint(format!("exports of '{}': {}", target_name,
                            if available.is_empty() { "(none)".to_string() } else { available.join(", ") })));
                    continue;
                }
                let local = n.local();
                if local_defs.contains(&local) {
                    diags.push(Diagnostic::error(&path, n.line, n.col, format!(
                        "imported symbol '{}' conflicts with a local definition in module '{}'",
                        local, self_name))
                        .with_hint("rename the local item or use `as <alias>`"));
                    continue;
                }
                list.push(ResolvedImport {
                    target: ti,
                    local: Some(local),
                    symbol: Some(n.name.clone()),
                    line: n.line,
                    col: n.col,
                });
            }
        }

        // 重复 / 冲突导入
        let mut seen_local: BTreeMap<String, (String, String)> = BTreeMap::new();
        for r in &list {
            let (local, sym) = match (&r.local, &r.symbol) { (Some(l), Some(s)) => (l, s), _ => continue };
            let src = files[r.target].module.name.clone();
            match seen_local.get(local) {
                Some((pm, ps)) if *pm == src && ps == sym => {
                    diags.push(Diagnostic::warn(&path, r.line, r.col,
                        format!("redundant duplicate import of '{}'", local)));
                }
                Some((pm, _)) => {
                    diags.push(Diagnostic::error(&path, r.line, r.col, format!(
                        "duplicate import '{}' (from '{}' and '{}')", local, pm, src))
                        .with_hint("use `as <alias>` to disambiguate"));
                }
                None => { seen_local.insert(local.clone(), (src, sym.clone())); }
            }
        }

        // 未使用导入（警告）
        let refs = files[fi].module.referenced_names();
        for r in &list {
            let used = match (&r.local, &r.symbol) {
                (Some(l), _) => refs.contains(l),
                _ => {
                    let exports = files[r.target].module.defined_names();
                    exports.intersection(&refs).next().is_some()
                }
            };
            if !used {
                diags.push(Diagnostic::warn(&path, r.line, r.col, format!(
                    "unused import from module '{}'", files[r.target].module.name))
                    .with_hint("remove it, or reference the symbol"));
            }
        }

        resolved.insert(fi, list);
    }

    // ===== 依赖边 =====
    let mut edge_set: BTreeSet<(String, String)> = BTreeSet::new();
    for (fi, list) in &resolved {
        let from = files[*fi].module.name.clone();
        for r in list { edge_set.insert((from.clone(), files[r.target].module.name.clone())); }
    }
    let edges: Vec<(String, String)> = edge_set.iter().cloned().collect();

    // ===== 环检测 + 拓扑顺序 =====
    let mut nodes: BTreeSet<String> = BTreeSet::new();
    for f in &files { nodes.insert(f.module.name.clone()); }
    let mut adj: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for n in &nodes { adj.insert(n.clone(), BTreeSet::new()); }
    for (a, b) in &edges {
        if let Some(s) = adj.get_mut(a) { s.insert(b.clone()); }
    }
    let cycles = find_cycles(&adj);
    let order = topo_order(&nodes, &edges);

    // 环 -> 诊断：定位到「闭合环」的那条 import 所在行
    for c in &cycles {
        if c.len() < 2 { continue; }
        let closer = c[c.len() - 2].clone();
        let head = c[0].clone();
        let (fi, line, col) = resolved.iter()
            .filter(|(fi, _)| files[**fi].module.name == closer)
            .flat_map(|(fi, list)| list.iter().map(move |r| (*fi, r)))
            .find(|(_, r)| files[r.target].module.name == head)
            .map(|(fi, r)| (fi, r.line, r.col))
            .unwrap_or((0, 1, 1));
        diags.push(Diagnostic::error(&files[fi].path, line, col,
            format!("cyclic module dependency: {}", c.join(" -> ")))
            .with_hint("break the cycle: move shared types into a third module, or keep the dependency one-directional"));
    }

    // ===== 链接：把导入符号（含传递依赖）克隆进导入方 =====
    let linked_idx = link_items(&files, &resolved, &mut diags);
    let mut linked: BTreeMap<PathBuf, Vec<Item>> = BTreeMap::new();
    let mut signatures: BTreeMap<PathBuf, u64> = BTreeMap::new();
    for (fi, items) in &linked_idx {
        if !files[*fi].is_input { continue; }
        linked.insert(files[*fi].path.clone(), items.clone());
        signatures.insert(files[*fi].path.clone(), dep_signature(&files, &resolved, *fi));
    }

    // ===== 模块视图 =====
    let mut modules: Vec<ModuleInfo> = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let name = f.module.name.clone();
        let empty: Vec<ResolvedImport> = Vec::new();
        let list = resolved.get(&i).unwrap_or(&empty);
        let mut deps: BTreeSet<String> = BTreeSet::new();
        let mut imports: Vec<String> = Vec::new();
        for r in list {
            deps.insert(files[r.target].module.name.clone());
            if let Some(l) = &r.local { imports.push(l.clone()); }
        }
        imports.sort();
        modules.push(ModuleInfo {
            name,
            version: f.module.version,
            file: f.path.clone(),
            is_input: f.is_input,
            exports: f.module.defined_names().into_iter().collect(),
            deps: deps.into_iter().collect(),
            imports,
        });
    }
    modules.sort_by(|a, b| a.name.cmp(&b.name));

    Report { modules, edges, order, cycles, diagnostics: diags, linked, signatures }
}

// ===================== 图算法 =====================

/// DFS 三色标记找环；返回去重后的环路径集合
fn find_cycles(adj: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Color { White, Gray, Black }

    fn dfs<'a>(
        n: &'a str,
        adj: &'a BTreeMap<String, BTreeSet<String>>,
        color: &mut BTreeMap<&'a str, Color>,
        stack: &mut Vec<&'a str>,
        out: &mut Vec<Vec<String>>,
        seen: &mut BTreeSet<String>,
    ) {
        match color.get(n).copied().unwrap_or(Color::White) {
            Color::Gray => {
                let start = stack.iter().position(|x| *x == n).unwrap_or(0);
                let mut cyc: Vec<String> = stack[start..].iter().map(|s| s.to_string()).collect();
                cyc.push(n.to_string());
                let mut key = cyc.clone();
                key.sort();
                key.dedup();
                if seen.insert(key.join("|")) { out.push(cyc); }
                return;
            }
            Color::Black => return,
            Color::White => {}
        }
        color.insert(n, Color::Gray);
        stack.push(n);
        if let Some(nexts) = adj.get(n) {
            for m in nexts { dfs(m.as_str(), adj, color, stack, out, seen); }
        }
        stack.pop();
        color.insert(n, Color::Black);
    }

    let mut color: BTreeMap<&str, Color> = BTreeMap::new();
    let mut stack: Vec<&str> = Vec::new();
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    // 借用：adj 的 key 生命周期需要 'a，这里用 indices 取引用
    let keys: Vec<&String> = adj.keys().collect();
    for k in keys {
        let ks: &str = k.as_str();
        if color.get(ks).copied().unwrap_or(Color::White) == Color::White {
            dfs(ks, adj, &mut color, &mut stack, &mut out, &mut seen);
        }
    }
    out
}

/// Kahn 拓扑排序：输出**构建顺序**（被依赖的模块排在前）。
///
/// 边 `(a, b)` 表示 a 依赖 b，因此 a 的入度按「依赖数」计数；
/// 先输出无依赖者，再逐层释放依赖它的模块。就绪集合用有序容器保证输出确定。
fn topo_order(nodes: &BTreeSet<String>, edges: &[(String, String)]) -> Vec<String> {
    let mut indeg: BTreeMap<String, usize> = nodes.iter().map(|n| (n.clone(), 0usize)).collect();
    // dependents[b] = 依赖 b 的模块集合
    let mut dependents: BTreeMap<String, BTreeSet<String>> =
        nodes.iter().map(|n| (n.clone(), BTreeSet::new())).collect();
    for (a, b) in edges {
        if !nodes.contains(a) || !nodes.contains(b) || a == b { continue; }
        if let Some(s) = dependents.get_mut(b) { if s.insert(a.clone()) {
            *indeg.get_mut(a).unwrap() += 1;
        } }
    }
    let mut ready: BTreeSet<String> = indeg.iter().filter(|(_, d)| **d == 0).map(|(n, _)| n.clone()).collect();
    let mut out: Vec<String> = Vec::new();
    while let Some(n) = ready.iter().next().cloned() {
        ready.remove(&n);
        out.push(n.clone());
        if let Some(ms) = dependents.get(&n) {
            for m in ms {
                if let Some(d) = indeg.get_mut(m) {
                    *d -= 1;
                    if *d == 0 { ready.insert(m.clone()); }
                }
            }
        }
    }
    out
}

/// 依赖闭包签名：任一依赖文件内容变化 ⇒ 依赖方的增量缓存失效
fn dep_signature(
    files: &[ModFile],
    resolved: &BTreeMap<usize, Vec<ResolvedImport>>,
    fi: usize,
) -> u64 {
    let mut set: BTreeSet<usize> = BTreeSet::new();
    let mut stack: Vec<usize> = vec![fi];
    while let Some(i) = stack.pop() {
        if !set.insert(i) { continue; }
        if let Some(list) = resolved.get(&i) {
            for r in list { stack.push(r.target); }
        }
    }
    let mut acc = String::new();
    for i in set {
        if i == fi { continue; }
        acc.push_str(&format!("{}:{:016x};", files[i].module.name, files[i].src_hash));
    }
    crate::fnv1a(&acc)
}

// ===================== 链接（符号内联） =====================

/// 为每个文件计算「需要从被导入模块克隆进来的条目」（含传递依赖）
fn link_items(
    files: &[ModFile],
    resolved: &BTreeMap<usize, Vec<ResolvedImport>>,
    diags: &mut Vec<Diagnostic>,
) -> BTreeMap<usize, Vec<Item>> {
    let mut out: BTreeMap<usize, Vec<Item>> = BTreeMap::new();
    for fi in 0..files.len() {
        let local_defs = files[fi].module.defined_names();
        let path = files[fi].path.clone();
        let empty: Vec<ResolvedImport> = Vec::new();
        let imports = resolved.get(&fi).unwrap_or(&empty);

        // 工作项：(来源文件下标, 源符号名, 本地可见名)
        let mut work: Vec<(usize, String, String)> = Vec::new();
        for r in imports {
            match (&r.local, &r.symbol) {
                (Some(l), Some(s)) => work.push((r.target, s.clone(), l.clone())),
                _ => for name in files[r.target].module.defined_names() {
                    work.push((r.target, name.clone(), name));
                },
            }
        }

        let mut done: BTreeSet<(usize, String)> = BTreeSet::new();
        let mut cloned: Vec<(String, Item)> = Vec::new();
        let mut steps = 0usize;
        while let Some((ti, sym, local)) = work.pop() {
            steps += 1;
            if steps > MAX_LINK_STEPS {
                diags.push(Diagnostic::error(&path, 1, 1,
                    format!("dependency closure too large (> {} symbols)", MAX_LINK_STEPS))
                    .with_hint("split the module or reduce cross-module type reuse"));
                break;
            }
            if !done.insert((ti, sym.clone())) { continue; }
            if local_defs.contains(&local) { continue; }
            let Some(item) = files[ti].module.find_item(&sym).cloned() else { continue; };

            // 传递依赖：该条目引用到的具名类型也要一起带过来
            let mut refs: BTreeSet<String> = BTreeSet::new();
            ast::collect_named_refs(&item, &mut refs);
            for d in refs {
                if local_defs.contains(&d) { continue; }
                if files[ti].module.find_item(&d).is_some() {
                    work.push((ti, d.clone(), d));
                    continue;
                }
                // 该符号由来源模块自己导入 —— 跟到它的来源
                if let Some(r) = resolved.get(&ti)
                    .and_then(|rs| rs.iter().find(|r| r.local.as_deref() == Some(d.as_str())))
                {
                    if let Some(s) = &r.symbol { work.push((r.target, s.clone(), d)); continue; }
                }
                // 兜底：全工程唯一定义
                let owners: Vec<usize> = (0..files.len())
                    .filter(|i| files[*i].module.find_item(&d).is_some()).collect();
                if owners.len() == 1 { work.push((owners[0], d.clone(), d)); }
            }

            cloned.push((local.clone(), item.renamed(&local)));
        }

        cloned.sort_by(|a, b| a.0.cmp(&b.0));
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for (l, _) in &cloned {
            if !seen.insert(l.clone()) {
                diags.push(Diagnostic::error(&path, 1, 1, format!(
                    "imported symbols collide on name '{}'", l))
                    .with_hint("use `as <alias>` to disambiguate"));
            }
        }
        out.insert(fi, cloned.into_iter().map(|(_, it)| it).collect());
    }
    out
}
