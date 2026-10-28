# xso-idlc — XuanShu IDL Compiler

XuanShu 跨语言 IDL 编译器：把一份 `.xso` 接口定义，生成 **Rust / C++ / anchor（C 头）** 等多语言绑定，并保证**两端二进制布局一致（ABI 稳定）**。

- 生成器版本 `GEN_VERSION = 3`
- 桥接 IR 版本 `IR_VERSION = 1`
- 二进制 IR 魔术字 `XSOIR`

---

## 1. 构建与运行

需要 Rust 工具链（建议 stable 最新）：

```bash
cd tools/xso-idlc
cargo build --release          # 产物在 target/release/xso-idlc
cargo test                     # 运行内置测试（含生成确定性、ABI diff 等）
```

无参数运行进入**交互式 REPL**；`--input ...`（旧式 flag）兼容旧调用。

---

## 2. 命令行

### 2.1 子命令

| 命令 | 作用 |
|------|------|
| `gen` | 从 IDL 生成 Rust / C++ / anchor / 文档 / 测试脚手架 |
| `check` | 仅校验磁盘产物是否最新（CI 门禁，不写盘） |
| `ir` | 打印 ABI 指纹 / IR 摘要（`--json` / `--bin` 可导出） |
| `diff` | 与基线做 ABI 差异比对（破坏性变更标记 `[break]`） |
| `deps` | **多文件依赖校验**：模块图 / 符号解析 / 环检测 / 拓扑构建顺序（见 §3.2） |
| `migrate` | 就地升级旧版 IDL 语法（幂等） |
| `sbom` | 导出 CycloneDX SBOM（记录产物 SHA-256） |
| `doctor` | 自检：示例可解析、生成确定性、缓存可写 |
| `interactive` / `ui` | 英文命令式交互 REPL（无参数等价） |
| `completions` | 生成 shell 补全脚本（bash/zsh/fish/powershell） |
| `version` | 打印版本 |
| `help` | 帮助（也支持 `xso-idlc help <cmd>` 与 `<cmd> --help`） |

### 2.2 通用选项

| 选项 | 说明 |
|------|------|
| `--input <path>` | IDL 输入，可重复（多个输入**并行**构建） |
| `--anchor <path>` | 布局锚点 C 头 |
| `--cpp-out <path>` | C++ 头文件 |
| `--rust-out <path>` | Rust 模块 |
| `--doc-out <path>` | Markdown ABI 文档 |
| `--test-out <path>` | 布局对拍测试脚手架 |
| `--emit-ir <path>` | 导出版本化 IR（JSON） |
| `--sbom <path>` | 导出 SBOM（也可用 `sbom` 子命令） |
| `--cache-dir <path>` | 增量缓存目录（默认 `.xso-cache`） |
| `-I, --include-dir <dir>` | 多文件依赖的模块搜索目录（可重复；自动包含各输入所在目录） |
| `--strict` | 严格模式：依赖告警（如未使用导入）升级为错误 |
| `--order` | 只打印拓扑构建顺序（`deps` 子命令，供构建系统消费） |
| `--jobs <N>` | 并行构建线程上限（默认：全部核心） |
| `--no-cache` | 禁用增量缓存，强制全量重建 |
| `-h, --help` | 帮助（全局或子命令级） |
| `-V, --version` | 版本 |

### 2.3 示例

```bash
# 生成全部后端
xso-idlc gen --input src/examples/fs.xso --rust-out build/fs_gen.rs --cpp-out build/fs.h

# 多个输入并行 + 限制线程
xso-idlc gen --input a.xso --input b.xso --cpp-out out --jobs 4

# CI 门禁：校验产物是否最新
xso-idlc check --input src/examples/fs.xso

# 导出二进制 IR 并做离线 diff
xso-idlc ir --input net.xso --bin net.irbin
xso-idlc diff --input new.xso --base old.irbin

# 交互式（英文命令）
xso-idlc            # 进入 REPL，输入 gen --input ... 等
```

---

## 3. IDL 语法

```xso
@module(network)
@version(1)

enum Status : u32 {
    Ok = 0
    NotFound = 1
    Err = 2
}

error IoError : i32 {
    Again = 1
    WouldBlock = 2
}

struct SockAddr {
    family: u16
    port: u16
    addr: Vec<u8>
    name: OwnedString
}

@opaque
struct Connection { /* 字段对外隐藏，只能指针传递 */ }

interface Transport {
    read(@borrowed self, buf: @borrowed Vec<u8>) -> result<usize, IoError>;
    send(@owned self, data: Vec<u8>) -> result<unit, IoError>;
}

type Handle = @owned Connection
```

顶层条目：`enum` / `error` / `struct` / `interface` / `type`（别名）。
- `enum` 底层类型默认 `u32`；`error` 底层类型默认 `i32`。变体必须显式 `name = 整数`。
- `struct` 可带泛型参数：`struct Wrapper<T, U> { ... }`（见 §5）。
- 行尾可用 `;` 或 `,` 分隔；顶层 `use ...;` 为跨模块导入声明（单文件编译时忽略，多文件工程下参与依赖校验，见 §3.2）。

### 3.1 嵌套数据结构模型

工具**原生支持嵌套 / 层级数据结构**：结构字段可以是另一个具名结构（按值内嵌）、以结构为元素的数组 / 可选 / 关联容器，以及通过容器实现的递归类型。所有嵌套在 Rust 与 C++ 两端按统一布局规范收敛，并带 `offset_of` 静态断言保证一致。

- **按值内嵌结构**：`struct B { inner: A }` —— `A` 直接作为 `B` 的内存一部分（自然对齐展开）。
- **容器内的结构**：`Vec<A>` / `Option<A>` / `Map<K, A>` / `Set<A>` —— 边界统一收敛为 `XsoVec<A>` / `XsoVec<XsoPair<K,A>>`（见 §5.2）。
- **递归类型**：结构**不能**直接按值自引用（`A { next: A }` 会被 §6.3 判为循环依赖），但**通过容器可安全递归**：`struct Node { value: u32; children: Vec<Node> }` —— `Vec` 是间接引用，自然打断循环。
- **不透明结构的嵌套**：`@opaque` 结构只能按指针传递，因此作为字段时必须写成 `@owned Node` 或 `@borrowed Node`（见 §6.1）。

已验证的端到端示例见 `src/examples/nested.xso`：

```xso
@module(nested)
@version(1)

struct Point { x: i32; y: i32 }

// 按值内嵌另一个结构
struct Rectangle {
    top_left: Point
    size: Point
}

// 复合嵌套：结构数组 / 可选结构 / 关联容器(结构) / 不透明结构指针
struct Scene {
    name: OwnedString
    rects: Vec<Rectangle>
    active: Option<Rectangle>
    tags: Map<u32, Rectangle>
    root: @owned Node          // 不透明结构只能按所有权/借用传递
}

@opaque
struct Node {                 // 递归通过 Vec 容器打破（而非按值自引用）
    value: u32
    children: Vec<Node>
}
```

> 边界：嵌套本身无额外层数限制，但**类型嵌套深度上限为 32**（含泛型展开，见 §6.4）；按值内嵌的不透明类型、按值自引用循环等仍受 §6 约束。

### 3.2 多文件依赖（`use` / 模块图）

单个 `.xso` 内部自洽只是最低要求；**多个 `--input` 构成一个工程**，工具会先做跨文件依赖校验，任何错误都会中止生成：

```xso
// base.xso
@module(base)
@version(1)
enum Color : u32 { Red = 1, Green = 2 }
struct Point { x: i32, y: i32 }
```

```xso
// app.xso
@module(app)
@version(2)
use base::{Point, Color};        // 具名导入
use base::Point as P;            // 带别名（可选）
use base;                        // 整模块导入（全部导出符号可见）

struct Rect { a: Point, c: Color }
```

- **依赖必须显式声明**：跨模块引用只能来自顶层 `use`，不存在隐式全局命名空间。
- **模块解析**：被导入模块必须在输入集合中，或能在 `--include-dir`（以及各输入所在目录）里按 `<module>.xso` 找到。
- **符号解析**：被导入符号必须存在于目标模块的导出中（导出 = 该模块全部顶层条目）。
- **禁止**：自我导入、重复导入（同一本地名来自不同来源）、与本地定义冲突、导入符号互相碰撞。
- **依赖环**：模块级依赖成环则无法拓扑排序，直接报错并给出环路径。
- **未使用导入**：默认告警，`--strict` 升级为错误。

```bash
xso-idlc deps --input idl/app.xso --input idl/base.xso      # 打印模块图 / 边 / 顺序 / 诊断
xso-idlc deps --input idl/app.xso -I idl --strict            # 自动发现依赖 + 严格模式
xso-idlc deps --input idl/app.xso --input idl/base.xso --order # 只打印构建顺序
xso-idlc deps --input idl/app.xso -I idl --json deps.json      # 导出依赖图 JSON
xso-idlc gen  --input idl/app.xso --input idl/base.xso        # 通过校验后才生成
```

**链接语义**：被导入的符号（连同其传递依赖）会在校验通过后**按值克隆进导入方模块**，因此每个模块的产物都是自包含的；由于两端使用同一套布局算法，克隆出来的类型在源模块与导入方模块布局常量完全一致，跨模块传递时 ABI 不变。

---

## 4. 支持的属性（注解）与说明

| 属性 | 位置 | 说明 |
|------|------|------|
| `@module(<Name>)` | 顶层 | 模块名（决定 Rust 模块名 `<Name>_bridge` 等） |
| `@version(<N>)` | 顶层 | 模块版本（`u32`），参与缓存签名与 IR |
| `@opaque` | struct / interface | 不透明类型：字段对外隐藏，**只能按指针（借用/所有权）传递** |
| `@owned T` | 类型 / 字段 / 参数 / `self` | 转移所有权，边界收敛为 `XsoBox<T>`（带 Drop 壳，move-only） |
| `@borrowed T` | 类型 / 字段 / 参数 / `self` | 借用视图，边界为裸指针/切片，**调用期间有效** |
| `@field(N)` | struct 字段 | 显式字段序号，保证字段重排后 ABI 不变（同 struct 内必须唯一） |
| `@method(N)` | interface 方法 | 显式方法序号，保证方法增删/重排后 ABI 不变（同 interface 内必须唯一） |

> 说明：`@owned` / `@borrowed` 既可作为类型前缀（`@owned T`），也可出现在方法上（`@owned self` / 参数 `@owned x: T`）。

### 已识别但**暂未生效**的注解（规划中）

`@doc(...)`、`@available`、`@added`、`@removed` 已被词法与解析层识别（不会报错），但当前**不接入生成或版本演进校验**，属于保留语法。请勿依赖其语义。

---

## 5. 类型系统与边界载体

### 5.1 原始类型

`u8 u16 u32 u64 i8 i16 i32 i64 bool unit`
（`unit` 即空值 / `void`）

### 5.2 复合类型 → 跨语言收敛规则

为保证 Rust 与 C++ 两端布局一致，所有“非平凡”类型在 FFI 边界统一收敛为**规范载体**：

| IDL 写法 | Rust 载体 | C++ 载体 | 边界宽度 |
|----------|-----------|----------|----------|
| `Vec<T>` | `XsoVec<T>` | `xso::Vector<T>` | 16 字节（指针+长度） |
| `Option<T>` | `XsoOption<T>` | 带标签结构 | 16 字节 |
| `Map<K,V>` | `XsoVec<XsoPair<K,V>>` | 同上（展开为扁平向量） | 16 字节 |
| `Set<T>` | `XsoVec<T>` | `xso::Vector<T>` | 16 字节 |
| `result<T,E>` | `i32` 状态码 + 可选 `*mut T` 输出指针 | `int32_t` + `out*` | — |
| `@owned T` | `XsoBox<T>`（带 Drop 壳，禁止拷贝） | `XsoBox<T>` | 16 字节 |
| `@borrowed T` | 切片/裸指针视图 | 裸指针/视图 | 8/16 字节 |
| `BorrowedStr` | `&str` | `xso::string_view` | 16 字节 |
| `OwnedString`（`String`） | `String`（`alloc::string::String`） | `xso::String` | 24 字节 |
| 普通具名 struct/enum/error | `#[repr(C)]` 结构 | `struct` | 自然对齐 |

> C++ 侧假定存在运行时类型 `xso::Vector<T>`、`xso::string_view`、`xso::String`、`xso::Result<T,E>`，需由内核/运行时提供相应头文件与实现。

### 5.3 泛型单态化

`struct Wrapper<T>` 在语义阶段按实际使用的类型实参**展开为具体结构** `Wrapper__u32`，泛型应用 `Wrapper<u32>` 改写为单态名。这样下游只看到普通具名类型，跨语言布局天然一致；嵌套泛型有界不动点迭代展开（上限 16 轮）。**泛型元数必须与定义一致**。

---

## 6. 边界与限制（不支持 / 约束）

### 6.1 不透明类型（opaque）

- **不能按值嵌入**到 struct 字段（会按值拷贝布局）——必须用 `@owned Box<T>` 或 `@borrowed T`。
- **不能按值作为接口参数**——必须 `@borrowed` 或 `@owned`。
- **不能按值作为接口返回值**——必须用 `result<@owned T, E>` 或 `result<@borrowed T, E>`。
- 违反上述任一条，语义检查直接报错并给出修复提示。

### 6.2 错误码约定

- `error` 枚举中**值 `0` 被保留为“成功”**，任何变体不得定义为 `0`；
- 同一 `error` 内错误码**不得重复**。

### 6.3 结构依赖

- 禁止**按值循环引用**的 struct（`A { b: B }` 且 `B { a: A }`）——用 `@owned`/`@borrowed`/容器打断循环。

### 6.4 资源与规模限制

| 限制 | 上限 |
|------|------|
| 类型嵌套深度 | 32 |
| 单文件条目数 | 4096 |
| 词法 token 数 | 200_000 |
| 标识符长度 | 128 字节 |
| 泛型单态迭代 | 16 轮 |

标识符不得含控制字符；重复定义（跨条目同名）、重复字段/方法/变体名、重复 `@field(N)`/`@method(N)` 序号均报错。

### 6.5 明确**不支持 / 暂未实现**

- **语言绑定**：仅生成 Rust、C++、anchor（C 头）、Markdown 文档、布局测试脚手架。Python / Go / 其他语言的绑定需要额外胶水（不在本工具范围）。
- **关联容器与集合是“扁平向量”**：`Map`/`Set` 收敛为 `XsoVec`，不是平衡树/哈希表，仅用于跨边界传输，不提供容器算法。
- **接口（interface）**：生成 Rust 安全 `trait` + 稳定 ABI 的 `VTable`（C++ 侧生成类包装）；但**不生成默认的 trampoline 实现**，需要业务侧手动把实现接线到 VTable。
- **`@doc` / `@available` / `@added` / `@removed`**：已解析但**不生效**（见 §4）。
- **数值范围 / 枚举底层宽度**：不校验整型溢出或枚举底层类型宽度是否足够容纳变体值。
- **手动改产物**：增量缓存以“源文本哈希 + IR 版本 + 生成器版本 + 产物清单”为键；若手动修改了某个产物文件，**缓存不会感知其内容变化**（源未变则仍判命中、不重写）。不要手工编辑生成产物后依赖工具回写。
- **C++ 运行时依赖**：C++ 产物依赖 `xso` 运行时类型（`xso::Vector` 等），本工具不自带该运行时实现。
- **跨机器 / 分布式并行**：`--jobs` 仅限本进程内多线程并行，不做跨节点构建。

---

## 7. 输出产物说明

| 后端 | 内容 |
|------|------|
| Rust (`--rust-out`) | `mod <module>_bridge { ... }`：repr(C) 结构、枚举、错误、XsoVec/Box/Option 载体、接口 trait + VTable、布局静态断言 |
| C++ (`--cpp-out`) | 头文件：结构/枚举/错误、Xso* 载体、`interface` 类包装 + VTable |
| anchor (`--anchor`) | C 头：布局断言（`static_assert` / `offsetof`）与 `XSO_LAYOUT_*` 宏，供内核侧核对偏移 |
| doc (`--doc-out`) | Markdown ABI 参考（模块/枚举/错误/结构/接口表格） |
| test (`--test-out`) | Rust 布局对拍测试（校验生成结构的 `size_of`/`align_of`） |
| IR (`--emit-ir`) | 版本化 JSON IR（含 ABI 指纹、布局清单），用于 diff |
| SBOM (`--sbom`) | CycloneDX 1.4，记录各产物 SHA-256 |

---

## 8. 确定性 / 缓存 / 并行

- **确定性**：有序容器 + 幂等生成（同输入多次生成字节一致）。
- **增量缓存**：默认开启。解析**之前**先按源文本 FNV-1a 哈希比对；命中则连解析/语义/生成都跳过（大 IDL 零开销）。键为“源哈希 + IR 版本 + 生成器版本 + 期望产物清单”，任何变化自动失效。可用 `--no-cache` 强制全量重建。
- **并行构建**：多个 `--input` 之间并行（受 `--jobs` 限流）；单个输入内部的多个后端也并行；任一后端 panic 被隔离为错误而非崩溃进程。

---

## 9. ABI 稳定性与 diff

- 每个模块有 **ABI 指纹**（FNV-1a），随布局/IR 版本/生成器变化而变。
- `diff --base <file>` 接受 `.xso` / `.ir.json` / `.ir.bin` 三种基线：
  - `.xso`：实时构建 IR；
  - `.json`：提取指纹快速比对；
  - `.bin`：离线反序列化做**真实**布局 diff。
- 破坏性变更以 `[break]` 前缀标记，并返回**非零退出码**，适合接入发布门禁。

---

## 10. 相关文档

- `docs/industrialization.md`：工业化设计（确定性、缓存、事务写盘、并行）。
- `docs/optimization.md`：优化路线与剩余项。
