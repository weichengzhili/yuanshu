# xso-idlc 工业化改造规范（Industrialization Spec）

| 项 | 内容 |
|---|---|
| 文档版本 | 1.0 |
| 状态 | 生效（Step 1 起逐步落地） |
| 适用范围 | `tools/xso-idlc` 生成器 + 其产出的中间桥接层（Rust / C++ / anchor / doc / test） |
| 关联 | `dtolnay/cxx` 的“共享中间布局 + 规范载体”桥接思想 |
| 规范性用语 | **MUST**（强制）/ **SHOULD**（推荐）/ **MAY**（可选） |

---

## 1. 目标

把 xso-idlc 的中间层从“能跑（demo）”提升到“可信边界（工业级正式环境）”，满足：

- **安全**：跨语言边界是唯一信任边界；所有 `unsafe` 收敛、可审计；对端输入一律不信任。
- **性能**：无隐式全量拷贝；热路径可预测；容器/字符串使用零拷贝视图语义。
- **稳定**：布局漂移、版本不匹配、非法输入均**快速失败**而非静默内存损坏。
- **可维护**：单一真相（IDL），生成代码可回归、可校验、可增量。

---

## 2. 架构与信任边界

```
        ┌─────────────────────────── Rust 侧（内核/服务） ───────────────────────────┐
        │  ergonomic trait (安全、易用)  ──(trampoline 胶水, unsafe 唯一落点)──► VTable │
        │                                                                              │
        │  XsoVec / XsoPair / XsoBox / XsoOption   ← 中间布局规范载体（唯一 ABI）      │
        └───────────────────────────────────┬──────────────────────────────────────────┘
                                            │  extern "C"  (稳定 ABI, 无语言原生容器)
        ┌───────────────────────────────────┴──────────────────────────────────────────┐
        │  C++ 侧：公共包装方法(易用) ──(marshalling)──► VTable 调用                    │
        │  XsoVec / XsoPair / XsoBox / XsoOption  （与 Rust 逐字节一致）                │
        └──────────────────────────────────────────────────────────────────────────────┘
```

**边界规则**
- 边界两侧 **MUST** 只使用生成的规范载体；语言原生 `Vec / String / Optional / Box / unique_ptr` **MUST NOT** 直接跨越边界。
- 所有 `unsafe` **MUST** 集中在生成的 trampoline 胶水与规范载体的 `from_raw/to_raw` 中；业务实现代码 **MUST** 保持 safe。
- 对端传入的指针、长度、判别式、句柄 **MUST** 在进入边界时校验。

---

## 3. 中间布局规范（Converged Bridge ABI）

### 3.1 规范载体（两端各生成一份，逐字节一致）

| 载体 | Rust | C++ | 布局 | 语义 |
|---|---|---|---|---|
| `XsoVec<T>` | `#[repr(C)] { ptr: *const T, len: usize }` | `template<class T> struct XsoVec { const T* ptr; size_t len; };` | 16B / align 8 | 序列/切片/字符串/集合，**借用视图，不转移所有权** |
| `XsoPair<K,V>` | `#[repr(C)] { key: K, val: V }` | `template<class K,class V> struct XsoPair { K key; V val; };` | `sizeof(K)+padding+V` | 关联容器键值对 |
| `XsoBox<T>`（规划） | `#[repr(C)] { ptr: *mut T, drop: Option<unsafe extern "C" fn(*mut T)> }` | `template<class T> struct XsoBox { T* ptr; void (*drop)(T*); };` | 16B / align 8 | **所有权转移**，接收方可按 `drop` 安全释放 |
| `XsoOption<T>`（规划） | `#[repr(C)] { tag: u8, value: MaybeUninit<T> }` | 对应 tagged struct | 依 T | 可选值，`tag=0` 表示 None |

### 3.2 布局规则
- 所有共享结构体 **MUST** 为 `#[repr(C)]`（自然对齐）；**MUST NOT** 使用 `packed`（未对齐访问是即时 UB，且破坏性能）。
- 字段顺序 **MUST** 由 IDL 唯一决定；Rust 与 C++ 必须同序同类型。
- 端序：同机 IPC **MAY** 依赖宿主端序；跨机/落盘 **MUST** 显式小端并做序列化层。
- 每个结构体 **MUST** 生成 `size / align / offset` 编译期断言（anchor）。

### 3.3 所有权与生命周期
- `XsoVec` 为**借用**：调用期间内存在调用方存活；被调方 **MUST NOT** 释放其 `ptr`，如需保留 **MUST** 深拷贝。
- `XsoBox` / 带 drop 壳的 `XsoVec` 为**所有权转移**：接收方 **MUST** 调用 `drop` 且仅调用一次。
- 每个接口方法 **MUST** 文档化参数所有权（借用/转移）与缓冲存活期。

---

## 4. ABI 版本与校验和（校验）

- **ABI 指纹**：对“布局描述符”（所有 struct 的 size/align/offset + 枚举取值 + VTable 签名）做哈希（xxHash/Blake3），生成 `XSO_ABI_CHECKSUM_<module>`，Rust 与 C++ 两侧 **MUST** 一致。
- **VTable 头部**（规划）：首三字段固定为
  - `abi_magic: u64`（模块魔数）
  - `abi_version: u32`（语义化版本）
  - `abi_checksum: u32`（ABI 指纹）
  调用方 **MUST** 在首次调用前校验三者；不匹配 **MUST** 失败退出，禁止静默运行。
- **版本规则**：破坏性布局/签名变更 **MUST** 递增主版本；新增可选方法 **SHOULD** 追加在末尾并保留旧槽位（或使用能力位图）。
- **IDL 版本**：`@version(n)` 变更 **MUST** 触发兼容性检查与文档更新。

---

## 5. 安全规范

| 编号 | 要求 | 级别 |
|---|---|---|
| S1 | `unsafe` 仅存在于生成的 trampoline 与规范载体转换；业务侧 zero-unsafe | MUST |
| S2 | 枚举判别式 **MUST** 经 `from_code`/`valid` 校验后再转换，禁止裸 `static_cast`/`transmute` | MUST |
| S3 | 成功码 `0` **MUST** 保留；错误枚举 **MUST NOT** 定义 `0` 变体（语义检查强制） | MUST |
| S4 | `XsoVec.len` **MUST** 校验上限（`XSO_MAX_BUF`）且 `ptr` 非空或 `len==0` | MUST |
| S5 | `out: *mut T` **MUST** 非空；被调方返回成功前 **MUST** 已写入 | MUST |
| S6 | `self_` **MUST** 带类型标签（magic + type-id）校验，防类型混淆 | SHOULD |
| S7 | `Fd/Socket` 等句柄 **SHOULD** 采用 `{index, generation}` 防回收复用 | SHOULD |
| S8 | 字符串若作路径/名字，**MUST** 校验 UTF-8 或显式标注为字节串 | SHOULD |
| S9 | 边界异常 **MUST** fail-closed（abort/返回明确错误），禁止未定义行为 | MUST |
| S10 | 跨分配器释放 **MUST** 通过 drop 壳（`XsoBox`/`XsoVecOwned`） | MUST |

---

## 6. 性能规范

- P1：容器/字符串参数 **MUST** 为零拷贝视图（借用），仅所有权转移时才复制。
- P2：热路径 **SHOULD** 支持批量 IO（`iovec`/`XsoVec<XsoVec<u8>>`）减少跨界次数。
- P3：结构体 **MUST NOT** packed；必要压缩仅在线缆/落盘序列化层执行。
- P4：VTable 调用开销 **MAY** 用“能力位图 + 直接导出热方法”优化。
- P5：生成阶段 **MUST** 支持并行（已落地）与增量（已落地），大 IDL 线性扩展。

---

## 7. 稳定性与可观测性

- T1：生成器错误 **MUST** 集中收集、带文件/行号上下文、一次上报（已落地）。
- T2：增量为内容级（未变不写）（已落地）。
- T3：运行期 ABI 自检 **SHOULD**：哨兵输入跑一遍方法，检测布局/判别式漂移。
- T4：边界错误 **SHOULD** 带稳定错误码与可选诊断字符串，便于定位。
- T5：**MUST** 提供资源上限（消息大小、并发数、超时）防 DoS。

---

## 8. 测试与 CI

- C1：每个 `.xso` 生成的 Rust **MUST** 通过 `rustc --crate-type lib` 编译（布局断言即单测）。
- C2：C++ 产物 **MUST** 通过 `-Wall -Werror` 编译并链接 anchor 断言。
- C3：**SHOULD** 加入“Rust 实现 ↔ C++ 调用”的往返 FFI 集成测试。
- C4：**SHOULD** 对解析器/生成器做快照测试（golden files）与模糊测试。
- C5：CI **MUST** 覆盖 `fs/net/kv` 三个示例的全后端生成与编译。

---

## 9. 现状差距矩阵

| 能力 | 现状 | 目标 | 差距 |
|---|---|---|---|
| 规范载体收敛 | `XsoVec`/`XsoPair` 已两端生成 | 补齐 `XsoBox`/`XsoOption` | 部分 |
| 结构体对齐 | 已去 packed，自然对齐 | 保持 + CI 断言 | ✅ |
| 多线程/集中错误/增量 | 已落地 | 保持 | ✅ |
| 复杂结构 | `Map`/`Set` 已支持 | 递归/载荷枚举 | 部分 |
| 枚举判别式校验 | 仍裸 `static_cast` | `from_code` + fail-closed | ❌ |
| 0 保留语义 | 未强制 | 语义检查报错 | ❌ |
| ABI 版本/校验和 | 仅编译期 anchor | 运行期 magic/version/checksum | ❌ |
| 安全 trampoline | 手填 VTable | 自动生成、unsafe 收敛 | ❌ |
| 所有权 drop 壳 | 裸指针 | `XsoBox`/owned `XsoVec` | ❌ |
| 类型标签/句柄代次 | 无 | magic + generation | ❌ |
| 尺寸上限/DoS 防护 | 无 | `XSO_MAX_BUF` + 超时 | ❌ |

---

## 10. 分阶段实施计划

### Phase 0 — 基线（已完成）
- [x] 生成器可编译；5 后端可运行
- [x] 生成物可独立编译（Rust）
- [x] 多线程 / 集中错误 / 增量

### Phase 1 — 边界安全止血（进行中）
- [x] 去 packed，自然对齐
- [x] `XsoVec`/`XsoPair` 规范载体 + `Map`/`Set` 收敛
- [ ] **S2 枚举判别式校验**：Rust `from_code`、C++ `valid/from_code`，替换裸 `static_cast`
- [ ] **S3 保留 0**：语义检查错误枚举禁止 `0`
- [ ] **S4/S5** `XsoVec`/`out` 边界校验 + `XSO_MAX_BUF`
- 验收：非法码/超长长度/空 `out` 全部 fail-closed；生成物编译通过。

### Phase 2 — 所有权与内存安全
- [ ] `XsoBox<T>`（drop 壳）与 owned `XsoVec<T>`（cap + drop）
- [ ] 跨分配器释放闭环测试
- [ ] `self_` 类型标签（magic + type-id）
- 验收：所有权转移/释放无泄漏、无双重释放；类型混淆被拦截。

### Phase 3 — ABI 版本与校验和
- [ ] ABI 指纹（布局描述符哈希）两侧一致
- [ ] VTable 头部 `magic/version/checksum` + 调用前校验
- [ ] 运行期 ABI 自检
- 验收：故意改一处布局 → 运行期立即报错而非静默损坏。

### Phase 4 — 性能
- [ ] 零拷贝视图语义固化（借用不复制）
- [ ] 批量 IO（iovec）
- [ ] 热方法直接导出 / 能力位图
- 验收：基准测试跨界次数与拷贝量下降。

### Phase 5 — 工程完备
- [ ] 递归/载荷枚举等复杂类型
- [ ] 快照测试 + 模糊测试 + 往返 FFI CI
- [ ] 资源上限/超时/可观测性
- 验收：CI 全绿；故障可定位；DoS 场景有界。

---

## 11. 附录：提交前检查清单

- [ ] 新增/修改 IDL 类型 → `layout_of` 与两端 `ffi_ty` 同步，anchor 断言通过
- [ ] 新增错误码 → 不含 `0`，`from_code` 覆盖
- [ ] 新增容器 → 使用规范载体，禁止原生容器跨界
- [ ] 生成物 `rustc` / `g++ -Werror` 编译通过
- [ ] 增量生成无多余写入（`up-to-date` 日志）
- [ ] 文档（doc 后端）与 IDL 版本一致
