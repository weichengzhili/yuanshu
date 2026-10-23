//! IDL 迁移工具：把旧语法自动升级到当前语法，并列出每条被应用的规则。
//!
//! 无三方依赖（手写扫描式替换，不用正则）。规则是幂等的：
//! 对已迁移的文件再跑一次不会产生任何改动。

/// 一次迁移的结果
pub struct Migration {
    pub content: String,
    /// 被应用的规则描述
    pub applied: Vec<String>,
    /// 是否发生了改动
    pub changed: bool,
}

/// 执行全部迁移规则
pub fn migrate(src: &str) -> Migration {
    let mut out = src.to_string();
    let mut applied: Vec<String> = Vec::new();

    // 规则 1：result<(), E> -> result<unit, E>（unit 才是本 IDL 的 Unit 写法）
    if out.contains("result<(),") {
        let n = out.matches("result<(),").count();
        out = out.replace("result<(),", "result<unit,");
        applied.push(format!("unit-return: result<(), -> result<unit, ({} 处)", n));
    }

    // 规则 2：@module NAME -> @module(NAME)
    let (o, n) = wrap_annotation(&out, "module");
    if n > 0 {
        out = o;
        applied.push(format!("module-paren: @module NAME -> @module(NAME) ({} 处)", n));
    }

    // 规则 3：@version N -> @version(N)
    let (o, n) = wrap_annotation(&out, "version");
    if n > 0 {
        out = o;
        applied.push(format!("version-paren: @version N -> @version(N) ({} 处)", n));
    }

    // 规则 4：旧的所有权写法 owned T / borrowed T（无 @）-> @owned T / @borrowed T
    for kw in ["owned", "borrowed"] {
        let needle = format!(" {} ", kw);
        if out.contains(&needle) {
            // 只处理类型位置（前面是 ': ' 或 '<' 的情况），保守处理 ': owned '
            let from = format!(": {} ", kw);
            let to = format!(": @{} ", kw);
            if out.contains(&from) {
                let n = out.matches(&from).count();
                out = out.replace(&from, &to);
                applied.push(format!("ownership-at: ': {} ' -> ': @{} ' ({} 处)", kw, kw, n));
            }
        }
    }

    Migration { changed: !applied.is_empty(), content: out, applied }
}

/// 把 `@<name> <ident>` 改写成 `@<name>(<ident>)`；已是括号形式则跳过
fn wrap_annotation(src: &str, name: &str) -> (String, usize) {
    let pat = format!("@{} ", name);
    let mut out = String::new();
    let mut rest = src;
    let mut count = 0usize;

    while let Some(idx) = rest.find(&pat) {
        let before = &rest[..idx];
        let after = &rest[idx + pat.len()..];
        if after.starts_with('(') {
            // 已是新写法
            out.push_str(before);
            out.push_str(&pat);
            rest = after;
            continue;
        }
        let end = after
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(after.len());
        let ident = &after[..end];
        if ident.is_empty() {
            out.push_str(before);
            out.push_str(&pat);
            rest = after;
            continue;
        }
        out.push_str(before);
        out.push_str(&format!("@{}({})", name, ident));
        rest = &after[end..];
        count += 1;
    }
    out.push_str(rest);
    (out, count)
}
