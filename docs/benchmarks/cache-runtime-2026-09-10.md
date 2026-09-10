# 历史缓存运行时测量（2026-09-10）

> 历史数据，当前未复测；测试/基准源码已删除。来源提交和复现限制见 [档案说明](README.md)，不作为当前交付验证证据。

基准：78ec5ef；优化：04640c1。相同 Rust 1.96.1、aarch64-apple-darwin、默认 features，构建命令 `cargo build --release -p agent-app --locked`。不包含同时进行的工具自动注册/工具 crate 重构。

真实 CLI 连接本地假服务，清空继承环境，使用合成文件与固定 SSE 响应。每个场景重复 3 次，下表为中位数。没有调用计费 API。

| 场景 | 请求数 | 改造前 JSON 字节 | 改造后 JSON 字节 | 变化 | 完整前缀保留：前 → 后 |
|---|---:|---:|---:|---:|---|
| 两轮工具循环 | 2 | 40,706 | 33,418 | -17.90% | 0/1 → 1/1 |
| 20 轮读取 | 20 | 3,379,419 | 3,015,387 | -10.77% | 0/19 → 18/19 |
| 跨进程恢复 | 3 | 68,103 | 57,511 | -15.55% | 0/2 → 2/2 |
| 跨问题记忆 | 2 | 43,387 | 41,659 | -3.98% | 0/1 → 1/1 |
| 显式委托 | 4 | 57,083 | 50,091 | -12.25% | 0/2 → 2/2 |
| Graph 调度 | 4 | 73,153 | 58,649 | -19.83% | 0/2 → 2/2 |

前缀统计比较同一 actor 的请求 messages，不跨工具权限组比较。长场景仅有一次批量压缩造成前缀切换。字节与前缀比例均不是服务端 Token 命中率。

20 轮场景 CLI 本地耗时中位数 0.42s → 0.40s，差异较小，不据此承诺推理延迟收益。峰值 RSS 15.36 MiB → 16.75 MiB（增加 1.39 MiB）；新增账本/续接数据和较大稳定窗口存在资源代价。可按实际工作负载调低上下文高低水位。

主程序 6,175,424 → 6,274,960 字节，增加 99,536 字节（1.61%）。缓存分支未新增依赖或异步运行时。此处不宣称产物变小。

验证覆盖 usage、完整响应/流式响应、截断工具不执行、续接数据、旧检查点反序列化、普通循环、跨进程恢复、失败重试、记忆选取、完整请求大小、批量压缩、用户约束保留、累计 Token 预留、CLI 额度修改和缓存策略。

格式检查、workspace Clippy -D warnings、workspace 测试通过；agent-core 无默认 features、agent-app 无默认 features 测试通过。整合工具重构后另行执行全局检查。

后续用官网同一模型、同一 API Key 的逐请求数据核对实际输入/缓存读/输出用量；区分首轮、普通续跑、压缩及不同 actor。思考强度默认沿用服务商设置。输入 Token 命中率 = 缓存读取 Token / 全部输入 Token，不能与请求命中次数比例混用。

## 本地提交记录

| 提交 | 变更 |
|---|---|
| b5a7101 | fix(usage): report DeepSeek cache reads and weighted input hit ratio |
| 96fb6d3 | fix(runtime): pause incomplete model responses before tool execution |
| dbe218e | feat(model): persist provider continuation data across resume |
| 25197fa | perf(prompt): remove duplicate execution schemas and bound node previews |
| 9436e93 | feat(runtime): persist per-agent prompt snapshots and state deltas |
| a651905 | feat(input): bound memory retrieval and complete model request size |
| 84c55cf | feat(context): compact history at watermarks while preserving instructions |
| e4c9e41 | feat(budget): persist shared token reservations and output limits |
| 04640c1 | feat(provider): configure cache boundaries and reasoning policy |
| bdc9b33 | fix(config): preserve legacy Anthropic defaults and record cache benchmarks |

整合保留了另一批工具重构的未提交内容。整合后的 workspace 269 项测试通过，关闭 SQLite 后 79 项核心测试通过，Clippy -D warnings 与格式检查通过。
