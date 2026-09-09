# 调用树与运行时序

每次 `Agent::run` / `run_stream` 生成独立 `run_id`，同一会话的多次输入不会混在一起。
应用默认追加到 `agent-trace.jsonl`，可用 `RS_AGENT_TRACE_FILE` 指定路径。

在项目根目录离线查看，不需要 API key：

```sh
cargo run -p agent-app -- --trace-map
cargo run -p agent-app -- --trace-map /path/to/agent-trace.jsonl
```

交互会话内输入 `/trace` 查看当前日志中的调用树。节点按开始顺序排列，包含：

- `agent.run`：一次完整运行。
- `context.load` / `context.save` / `context.delete`：上下文读取、持久化、删除。
- `memory.search` / `memory.save`：长期记忆检索、会话摘要保存。
- `tools.definitions`：生成工具定义。
- `agent.step`：模型与工具循环的一轮。
- `model.prepare`：上下文快照与请求准备。
- `model.request`：具体 Provider、模型名称和轮次；包含请求转换、HTTP 与增量流解析。
- `tool.call`：工具名称和模型返回的 `call_id`。

调用关系示意（实际输出会在每行附上时间和用量）：

```text
agent.run
  context.load
  context.save
  memory.search
  tools.definitions
  agent.step {"loop_step":1}
    model.prepare
    model.request
    tool.call {"tool_name":"echo","call_id":"call-1"}
    context.save
  agent.step {"loop_step":2}
    model.prepare
    model.request
    context.save
```

## 时间、token 与缓存的含义

`[+Nms]` 是相对本次运行起点的偏移，耗时使用单调时钟，保留小数毫秒。
父节点耗时包含子调用、编排和追踪写入，不能将各层耗时再次相加。
模型节点另显示首个文本增量延迟（如果有文本）；流式回调仍随增量触发。

`token(in/out/total)` 分别为输入、输出和两者合计。父节点汇总其子树内的模型请求，
同一请求只计一次；跨层再相加会重复统计。`reasoning` 是输出 token 的子集。
这些是服务端报告的 token 数量，不是金额，也不是按字符估算的用量。

`cache` 取值为 `hit`、`miss`、`unknown` 或 `not_applicable`。
`read` / `write` 为缓存读取/写入的输入 token，`hits=H/R` 为已报告缓存状态的
R 次请求中有 H 次读取了缓存，`calls` 为全部模型请求数。父节点只要还有未报告的
请求，缓存整体状态就是 `unknown`，已知的命中次数仍会保留。

未报告的计数显示 `?`（JSON 中为 `null`）。某个子调用缺少一个计数时，该计数的
父级合计也为未知，避免把不完整统计误当作完整总量。无模型调用的本地节点用量为 0、
缓存为 `not_applicable`；这里的缓存指标专指模型服务的 prompt cache，
不将 SQLite、文件系统缓存或检索结果非空误当作模型缓存命中。

- OpenAI：读取 `prompt_tokens`、`completion_tokens`、`cached_tokens` 和
  `reasoning_tokens`。流式请求默认发送 `stream_options.include_usage=true`，
  包括 `choices=[]` 的末尾 usage 分片也会被读取。兼容服务若拒绝此参数，可设置
  `OPENAI_STREAM_USAGE=false`（或 `0`）；没有返回的用量仍显示未知，不会自动重试请求。
- Anthropic：`input_tokens` 不包含缓存，统一输入量由输入、缓存读取和缓存写入三项相加。
  任何一项缺失时统一输入量未知。`message_start` 与 `message_delta` 的用量按字段覆盖，
  不累加累计计数，缺省字段保留已有值。

协议依据：[OpenAI usage 说明](https://help.openai.com/en/articles/10478918)、
[Anthropic 流式事件说明](https://platform.claude.com/docs/en/build-with-claude/streaming)。

## 格式、错误与资源边界

保留原有 JSONL 事件，并关联当前 `run_id` / `span_id`。新增事件：

- `trace.span.started`：`run_id`、`session_id`、`span_id`、`parent_span_id`、
  `depth`、`operation`、`start_offset_ms`、`details`。
- `trace.span.completed` / `trace.span.failed`：`duration_ms`、`status`、`error`、
  `usage`、`total_tokens`、`model_calls`、`cache_hits`、`cache_reports`、`cache_status`。

工具错误会结束对应工具节点，原有“将工具错误反馈给模型并继续”的行为保持不变。
模型、存储、空响应、轮次超限和配置错误会结束当前活动节点与根节点。
流中的服务端错误及结束标记前断流按失败处理，不能将半段流标为成功。
失败模型请求未取得最终用量时显示未知。

异常退出或仍在执行的节点显示 `incomplete`；没有结束事件时不能得知最终耗时和用量。
查看器会提示并忽略末尾不完整 JSON，其他损坏行报错。旧版日志会跳过，不能补出缺失的调用关系。

核心追踪只保存活动调用栈和累计计数，不在内存保留整次运行的历史。
文件输出沿用每条事件写入后 flush 的策略，便于实时读取；不等于 fsync 的断电持久性保证。
新增节点增加了序列化和写入次数，没有宣称运行性能提升。文件仍按追加方式增长，需要按使用情况归档。
查看器单行最多读取 4 MiB、最多 100000 节点、最多 64 MiB 调用元数据、深度最多 64，
超限明确报错并要求拆分日志，不静默省略节点；限制不改变 Agent 的工具调用协议。

通用节点与聚合逻辑位于 `agent-core/src/trace`，Provider 协议用量解析位于
`agent-app/src/provider/*/usage.rs`，终端查看位于 `agent-app/src/trace_map`。
新增功能使用现有依赖，没有改变 SQLite features 和 release 编译配置。

## 本次验证记录

- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通过。
- `cargo test --workspace`：125 项测试通过。
- `cargo test -p agent-core --no-default-features`：15 项测试通过；现有
  `ContextStoreError::storage/serialization` 在关闭 SQLite 时有 dead_code 警告。
- 本机 HTTP 模拟服务验证两次模型请求、一次 echo 工具调用、`/trace` 和无 API key 的
  离线查看；汇总为 220 tokens、100 缓存读取 tokens、2/2 缓存命中。
- 同一工作区改动前后均执行 `cargo build --release -p agent-app --locked`，
  Rust 1.96.1、`aarch64-apple-darwin`、默认 features 和原 release 配置。
  主可执行文件由 5,756,384 增至 5,790,864 字节，增加 34,480 字节（约 0.60%）。
  基线包含工作区中原有未提交改动；未做运行时间或内存的对比基准。
