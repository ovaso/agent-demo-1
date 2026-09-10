# Review 核对与修复

修复基线为 `2f9c6c1`。本轮处理工具契约、文档证据、Runtime 错误分类和 Agent 恢复路径断言；没有修改数据库 schema、预算规则、工具权限或自动重试策略。

## 工具文案与旧检查点

已用真实 CLI 在临时数据库复现：只修改已保存工具及参数的 description，`/resume` 就报“模型或工具定义与检查点不一致”。同一输入在修复后恢复成功。

`ToolDefinition::same_contract` 现在比较工具名、read_only、参数数量、参数名及 required；说明文字、参数顺序、created_at 和 version 不属于执行契约。重复参数名被拒绝。参数当前均为字符串，未来引入类型、枚举或值域约束时必须纳入契约比较。

旧检查点无需 SQL 或格式迁移：恢复时直接使用新的兼容性比较。仍逐个验证保存的工具集合，缺失工具或执行约束变化继续拒绝；当前注册的新工具不会自动授予旧任务。保留旧任务的工具说明快照，仅沿用现有 created_at/version 刷新逻辑，新建任务使用当前说明。

## 错误类别与来源

移除内部 `RuntimeError::storage(impl Display)` 的统一字符串转换。内置路径分别返回 `Io`、可选 SQLite 的 `Database`、`Serialization`、`Trace`、`Memory`、`Model` 或 `Agent`，并实现 `Error::source()`。`TraceError` 也保留 I/O/JSON 的下一层来源。

调用方可检查 SQLite 错误码、I/O kind 或 JSON 错误类别；这不等于获得自动重试许可，副作用结果未知仍须核实。已有 `Storage(String)` 和 `Execution(String)` 变体保留给自定义调用方，但其本身无法提供 source。既有 ModelError、MemoryStoreError、ContextStoreError 内部仍可能是文本，本轮不声称已恢复这些包装器在更早边界丢失的原始原因。

**公开 Rust 接口变化：** RuntimeError 新增变体，且因持有不可克隆、不可结构比较的标准错误，不再派生 `Clone/PartialEq/Eq`。消费者应按变体及 source 判别；穷举匹配或依赖这些 trait 的外部代码需要适配。RuntimeError 本身不持久化；暂停原因仍保存字符串，旧检查点字段不变。

## 恢复路径断言

`agent-core/src/agent/` 中的 `expect` 与 `unreachable!` 已收敛为上下文明确的错误或已有 Option 语义。涉及图绑定/节点结算、计划依赖、协作消息、提示词历史、预算策略和工具分发。节点结算先检查活动图和节点，再取走协调者上下文；消息记录先确认存在，再增加序号。

多数原断言由同步代码的前置校验保证，不将断言数量当作已经证实的崩溃数量。CLI 对畸形历史帧与不存在活动节点的检查点返回 Invalid；未发现或声称复现全部断言的生产触发条件。

## 文档与历史数据

`implementation-progress.md` 改为当前能力与限制清单，移除将旧测试数量/阶段体积混作当前证据的描述。`validation-2026-09-10.md` 改为当前检查入口，历史报告通过明确 Git 提交查阅，不再链接已删除测试文件。

`docs/benchmarks/README.md` 标注各产物的产品提交、基准源码提交、报告提交和可复现限制；JSON 仅增加 `_provenance`，原始测量字段保留。当前 checkout 缺少测量程序，未重新运行这些历史基准，不能用其数字证明本轮性能。

## Review 中需要纠正或保留的结论

- **请求超时：** Cargo.lock 锁定 reqwest 0.13.4；其 blocking ClientBuilder 默认 Timeout 为 30 秒，Response 的 Read 也使用该超时。因此“Provider 完全没有超时”不准确。当前没有应用级显式配置，也没有覆盖整个流式响应的绝对总期限；持续返回数据的流可以延续更久。子进程捕获/等待确实没有执行期限。
- **3 MB 的测量口径：** 缓存报告的 20 轮约 3 MB 是累计模型请求 JSON，不是检查点写入量。全量检查点仍随数据规模遍历，但其当前耗时/I/O 影响需要独立测量。
- **性能策略：** 增量检查点、记忆索引和批量 trace 刷新没有实现。后续必须先建立固定输入基线，再定义事务恢复、缓存失效和异常退出日志保留语义。超时需要单独定义取消、进程回收和未知副作用处理，不能归为只影响性能的优化。

## 本轮验证

- `cargo fmt --all -- --check`、`cargo check --workspace`、`cargo check -p agent-core --no-default-features`、`cargo build --release -p agent-app --locked` 均通过。
- 临时 CLI 验证：仅改文案、参数重排、保存工具集合是当前集合子集，均允许恢复；工具名/只读权限/参数名/必填性变化、重复参数、参数缺失，均拒绝恢复。
- 畸形历史帧、缺失活动节点、无效检查点 JSON、损坏 SQLite 文件均返回明确错误并以状态码 1 结束，无进程 abort。
- `git diff --check` 通过；临时数据库已删除，没有保留测试、夹具、测量程序或调试日志；没有执行 `cargo test`。

这些有限输入只核对契约恢复和错误诊断，未覆盖完整图协作流程、真实模型 HTTP 或超时取消。不存在“全部功能已回归”的结论。

同一 `rustc 1.96.1`、`aarch64-apple-darwin`、默认 features 与 release 构建命令下，主程序从 **6,318,304** 增至 **6,335,824 字节**，增加 **17,520 字节（约 0.277%）**。依赖、features 和 release 配置未改；这是保留具体错误与诊断的体积代价。本轮未测量运行耗时、峰值内存或分配次数。
