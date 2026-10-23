# xso-idlc 深度优化路线图（10 个方面）

| 项 | 内容 |
|---|---|
| 版本 | 2.0 |
| 上级规范 | `docs/XSO-IDLC-INDUSTRIALIZATION.md` |
| 状态 | 本轮已落地 1/2/3/4/5/7/8/9 的主体；6/10 部分待续 |

图例：**✅ 已落地** · **🟡 部分** · **⬜ 待办**

---

## 1. 词法解析 ✅

| 目标 | 实现 | 位置 |
|---|---|---|
| 全 token span | `Token.span: (usize, usize)`（字节区间，由 `tokenize` 统一填充，不侵入各分支）；`CompileError.span` 并在 `expect/expect_ident/expect_int` 自动附加 | `lexer.rs`, `diag.rs`, `parser.rs` |
| 错误后继续解析 | 恐慌模式同步：`recover_item()` / `recover_member()` / `try_member()`；顶层条目、结构体字段、接口方法、枚举/错误变体**四处均带恢复**；`parse_with_recovery()` 一次返回全部错误 | `parser.rs` |
| UTF-8 标识符 | 标识符起始/续接接受非 ASCII 字节（`c >= 0x80`）+ `from_utf8_lossy` | `lexer.rs` |
| 完整错误信息 | `file:line:col` + **源码行** + `^` 列指示 + hint；`attach_source_lines()` 统一附加 | `diag.rs`, `main.rs` |

## 2. 语义分析 🟡（泛型待办）

| 目标 | 实现 |
|---|---|
| 拓扑排序 / 循环检测 | `check_cycles()`：仅按值嵌入的具名字段建边，`DFS` 三色标记，报错给出**环路径**（`A -> B -> A`），并提示用 `@owned/@borrowed` 断环 |
| 深度限制 | `check_type(..., depth)`，`MAX_TYPE_DEPTH = 32`，超限报错防栈溢出 |
| 符号表 | 重复定义检测（enum/error/struct/interface/alias）、重复字段/方法/变体检测、标识符合法性（长度 ≤128、禁控制字符） |
| 泛型支持 | ⬜ 远期（当前 `Vec/Option/Map/Set/Result` 为内建参数化类型） |

## 3. BridgeIR ✅

| 目标 | 实现 |
|---|---|
| 版本化 IR | `IR_VERSION = 1` + `BridgeIr.ir_version`；产物写入 `XSO_IR_VERSION` |
| 序列化 | `to_json()`：确定性 JSON（键有序），含布局/偏移/枚举/不透明/指纹 |
| diff | `diff(&other)`：类型布局、字段偏移、枚举大小、增删 → 输出 `[break]` / `[compat]` 分类；`--emit-ir` 落盘并与上次的**指纹**比对，输出 `+N -M` 行差异 |
| 确定性 | 所有 `HashMap/HashSet` → `BTreeMap/BTreeSet`，迭代顺序固定 ⇒ 生成幂等 |

## 4. Anchor ✅

| 目标 | 实现 |
|---|---|
| 算法版本 | `FINGERPRINT_ALGO_VERSION` 常量写入 anchor 头；算法升级即整体可控失效 |
| 自校验 | `XSO_ABI_CHECKSUM_<MODULE>`（FNV-1a 32 布局指纹）同时写入 **anchor / Rust / C++** 三处，可运行期比对 |
| 冲突检测 | 语义层符号表拒绝重名定义与重名成员 |
| 审计日志 | anchor 头 `audit:` 注释（module/version/ir_version/algo/fingerprint），**确定性**（不含时间戳） |

## 5. 代码生成 ✅

| 目标 | 实现 |
|---|---|
| 幂等 / 确定性 | 有序容器 + 统一缩进/空行策略 |
| 格式化 | 生成即规范排版，不依赖人工 |
| 代码检查 | `--check`：与磁盘逐字节比对，缺失/过期即非零退出（CI 门禁） |
| 增量写 | 内容未变不写盘（`up-to-date`） |

## 6. 测试 🟡

- ✅ `tests/parse/broken_multi.xso`：多错误样例（供错误恢复验证）
- ✅ `--check` 提供快照式 CI 门禁
- ⬜ 单测 / fuzz / 对拍（路线图 Step 6）

## 7. 安全 🟡（SBOM 待办）

| 目标 | 实现 |
|---|---|
| 输入验证 | 标识符长度上限、禁控制字符、未知类型/基元拒绝 |
| 资源限制 | 源文件 ≤ 8 MiB、记号数 ≤ 200k、条目数 ≤ 4096、类型嵌套 ≤ 32 |
| panic 隔离 | 生成阶段 `catch_unwind`，单后端 panic → 转为集中错误，不崩进程 |
| SBOM | ⬜ 待办 |

## 8. 性能 ✅

| 目标 | 实现 |
|---|---|
| 增量编译 | 内容级（未变不写）+ 哈希级（`.xso-cache`，输入哈希/IR 版本/生成器版本一致且产物齐全 → 整轮跳过） |
| 并行 | 各后端 `std::thread::scope` 并行生成；支持多 `--input` |
| 缓存 | `--cache-dir`（默认 `.xso-cache`） |

## 9. 可演进 🟡

- ✅ IR 版本（`IR_VERSION`）
- ✅ 兼容性检查：`diff()` 的 `[break]`/`[compat]` 分类 + 指纹比对（`--emit-ir`）
- 🟡 迁移工具：暂无独立工具，靠 `--emit-ir` + diff 人工/CI 判定

## 10. 稳定性 🟡

- ✅ panic 隔离（`catch_unwind`）
- ✅ CI 门禁（`--check`）
- ⬜ 回滚策略（属工程流程，非生成器职责）

---

## 验证状态

- 上一轮 `cargo build` 报 5 个错误：**4× `Token` 缺 `span` 字段** + **1× `Item::Enum(e) | Item::Error(e)` 或模式类型不匹配**。两处均已修正（Token 构造补 `span: (0,0)` 由 `tokenize` 回填；Enum/Error 拆分为独立分支）。
- 重新构建被放到后台执行，**编译结果尚未回收确认**。
