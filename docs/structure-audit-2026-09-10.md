# 结构审查与重构

本页记录 `2f9c6c1` 的审查结论。后续已修复工具文案兼容性、错误分类和恢复路径断言，并更正超时与基准口径；最新状态见 [review 修复记录](review-fixes-2026-09-10.md)。下列“后续问题”为当时发现项，不代表均仍未解决。

基线为 `c11ac13`，审查覆盖 workspace 的六个 crate，重点追踪 Runtime、CLI、Provider、上下文和持久化调用关系。本轮完成职责分组与重复实现收敛，不新增测试、测试夹具或基准程序，不运行 `cargo test`。

## 已处理的问题

| 问题 | 处理与归属 |
| --- | --- |
| 模块入口混用 `name.rs + name/` 和 `name/mod.rs` | 全部统一为有子模块使用 `目录/mod.rs`、叶子模块使用普通 `.rs`；迁移应用配置、Markdown 记忆和表格模块，规则写入根 `AGENTS.md`。 |
| Runtime 下 27 个内部模块平铺，难以辨认业务边界 | 收拢为 12 个顶层内部模块，其中 `execution`、`coordination`、`prompt`、`tools`、`budget`、`store` 分别负责执行、协作、模型输入、控制工具、额度和持久化。 |
| 委托/消息工具依赖规划工具中的公共调用类型与参数解析 | 提取 `tools/invocation.rs`，分发统一放在 `tools/mod.rs`，各工具组只负责各自处理逻辑。 |
| 操作者 API 与模型工具重复维护计划提交后的状态转换 | `coordination/plans.rs` 统一计划版本更新、旧消息失效、图绑定与进展记账；入口各自保留权限和安全边界校验。 |
| `context/mod.rs` 混合消息定义和历史容器 | 消息与角色移到 `context/message.rs`；历史裁剪和压缩仍由 Context 管理。 |
| CLI 会话同时做命令分发、装配与执行输出 | 命令分发移到 `cli/dispatch.rs`；`session.rs` 保留会话装配、启动与运行输出。模型配置单独放入 `config/provider.rs`。 |
| Provider 普通/流式请求重复 HTTP 编码、鉴权与发送 | 协议请求装配归入各自 `request.rs`；各自 `http.rs` 负责端点和鉴权；`transport.rs` 统一有界编码、发送、状态检查和普通 JSON 响应读取。流式解析仍逐增量回调。 |
| 内部可见性过宽、位置型返回值不清晰 | RunState 与预算内部字段限制到 Runtime；RunLease 后端句柄私有；应用 Provider 使用 `pub(crate)`；模型输入准备结果改为具名 `Prepared`。 |
| Anthropic 配置错误静默生效 | `ANTHROPIC_MAX_TOKENS` 的非法整数与零值现在报错，未设置仍默认 1024。此项是明确的行为修正。 |
| 文档引用已删除测试和过时入口 | 历史验证与重构报告加时间范围说明，环境文档更新模块路径；旧测试数量不再当作本轮验证证据。 |

`mod.rs` 写法本身与另一种写法没有语义优劣，本轮采用它是为了项目一致性。有界状态、事务边界、同步执行、工具顺序、序列化字段与公开 API 导入路径保持原设计。没有改依赖、features、release 配置或数据库 schema。

## 目录职责

```text
agent-core/src/agent/runtime/
  mod.rs                  Runtime 装配与公开类型导出
  control.rs              启动、暂停、取消、人工恢复
  workspace.rs            操作者计划与工作板 API
  state.rs                检查点数据、只读访问、状态有效性
  options.rs / error.rs   启动选项、公开错误
  serialization.rs        有界序列化
  execution/              driver、model、tools、graph
  coordination/           delegation、messages、delivery、graph、plans
  prompt/                 input、instructions、history、memory、view
  tools/                  分发、invocation、planning、agents、messages
  budget/                 RunLimits / RunBudget、steps、tokens
  store/                  RunStore / RunLease、memory、sqlite
```

跨组状态转换仍在同一个 Runtime 检查点事务内完成。分组用于明确代码归属，不把同一份运行状态拆成多个各自提交的服务，也不增加转发对象或新的 crate。

## 后续应优先处理的设计问题

1. **工具兼容性与说明文字耦合。** `tool/definition.rs::same_contract` 比较 description 和完整 Parameter，因此仅调整文案也会使 `execution/driver.rs` 拒绝恢复旧检查点。应把能力/参数约束与展示说明分开建模，并明确旧检查点迁移规则。当前 `run_check` 只有 fmt/clippy/test，缺少项目要求的 check；本轮保留其原定义，避免补充文案或参数说明导致旧任务无法恢复。这是待解决问题。

2. **完整检查点的写入成本随历史增长。** `store/sqlite.rs` 每次提交仍编码整个 RunState，并序列化会话投影；MemoryRunStore 仍复制整个状态。图历史、计划、消息都在同一检查点中。现有字节上限约束了规模，但没有消除全量遍历。若以后改成增量存储，需要同时定义原子性、恢复顺序和回收策略；本轮没有改这些语义。

3. **内存有界不等于 I/O 有界。** `memory/markdown/mod.rs::search_bounded` 限制结果数量和字节，但仍遍历目录、逐文件读取并匹配。大量记忆时，应该用固定数据先测量，再选择可失效的索引或可选后端；不能直接缓存而忽略用户编辑 Markdown 文件的可见性。

4. **耗时边界没有形成统一策略。** 应用 Provider 没有显式的请求截止配置，`agent-app-tool/src/process_output.rs` 排空管道并等待子进程，未设置工具执行期限。长请求或不退出的工具会持续持有同步运行位置。后续应区分取消模型调用、终止工具和“副作用结果未知”，不能简单超时后自动重试。

5. **两套执行入口仍有长期维护成本。** `Agent` 的轻量 runner 与可恢复 `Runtime` 共同复用模型调用逻辑，但仍分别推进循环、写上下文和处理工具。它们的能力边界不同，不能直接删除其中一个；应明确轻量入口的兼容承诺，新增行为时判断是否需要两条路径同步支持。

6. **追踪实时性与成本需要量化取舍。** `trace/mod.rs::FileTraceSink::record` 每条事件都会 flush；逐增量构建事件也有分配成本。批量刷新会改变异常退出时的日志保留范围，需先测量并明确约定。本轮保持现有策略。

7. **错误来源被压成字符串。** 多数 Runtime 内部错误通过 `RuntimeError::storage` 转成 String，部分序列化、模型输入和追踪失败也被归入 Storage，难以由调用方判断恢复动作。后续需要梳理错误分类和 source 保留方式，并考虑公开错误枚举及持久化暂停原因的兼容性。

这些问题并非都需要新抽象。下一轮优先处理工具契约兼容性和执行期限；性能问题先测量，避免仅凭代码形态判断收益。

## 验证与体积

- `cargo fmt --all -- --check`：通过。
- `cargo check --workspace`：通过。
- `cargo check -p agent-core --no-default-features`：通过。
- `cargo build --release -p agent-app --locked`：通过。
- `git diff --check`：通过；源目录未发现同名 `.rs` 与模块目录混用。
- 独立临时目录人工 CLI 检查：创建任务、调整预算至 12、暂停、另一个进程读取持久化状态、取消、清除会话均成功。检查点版本依次为 0 → 1 → 2 → 3；临时数据已清理。
- `ANTHROPIC_MAX_TOKENS=invalid` 与 `0` 均启动失败并返回明确错误。

上述检查没有请求真实模型，没有运行自动化测试，也没有覆盖两种协议的实际 HTTP 交互或复杂协作端到端行为。编译与有限人工检查不能证明全部行为等价。

同一工具链 `rustc 1.96.1`、`aarch64-apple-darwin`、默认 features、相同 release 命令下，`target/release/agent-app` 从 **6,318,064** 变为 **6,318,304 字节**，增加 **240 字节（约 0.0038%）**。未测量运行耗时、峰值内存或分配次数，不宣称性能提升。
