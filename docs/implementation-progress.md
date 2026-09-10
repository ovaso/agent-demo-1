# 自适应运行时当前实现状态

本页描述当前源码具备的能力，不把设计目标或删除测试前的历史结果当作当前验证证据。测试、夹具与基准源码已在 `f6e832c` 删除；当前验证方式以根 `AGENTS.md` 为准。

## 当前能力与代码归属

| 能力 | 实现位置 | 当前边界 |
| --- | --- | --- |
| 上下文与完整工具批次裁剪 | `agent-core/src/context/` | 消息数为软上限；工具批次和字节上限由 Runtime 管理。 |
| 可恢复 Loop、逐工具检查点 | `agent-core/src/agent/runtime/control.rs`、`execution/` | 未知副作用需人工核实或显式重试；恢复不重置预算。 |
| SQLite 与内存后端 | `agent-core/src/agent/runtime/store/` | SQLite 检查点与会话投影同事务提交；仍使用完整快照。 |
| Plan、Blackboard、受限只规划模式 | `agent-core/src/agent/planning/`、`blackboard/`、`runtime/tools/` | 计划交付后暂停，显式执行后沿用预算与验收要求。 |
| Loop/Graph 路由与节点结算 | `agent-core/src/agent/runtime/coordination/graph.rs`、`execution/graph.rs` | 同步顺序调度，区分模型报告、工具成功、零退出码与操作者验证。 |
| 委托与协作消息 | `agent-core/src/agent/runtime/coordination/` | 平面团队、共享 Provider；节点不能提升权限或递归创建团队。 |
| 预算、有限续期、Token 预留 | `agent-core/src/agent/runtime/budget/` | 根与节点共享总额度；恢复保留用量及续期历史。 |
| 提示词快照、增量与压缩 | `agent-core/src/agent/runtime/prompt/`、`context/compaction.rs` | 状态数据不改变权限；缓存命中与模型质量需服务端证据。 |
| CLI 与环境配置 | `agent-app/src/cli/`、`config/` | 启动时解析，支持离线状态读取；不改写进程全局环境。 |
| 模型适配 | `agent-app/src/provider/` | OpenAI 兼容与 Anthropic 各自转换协议，共享 HTTP 发送；增量输出。 |

使用方法见 [运行时 CLI](runtime-cli.md)、[环境配置](environment.md)、[预算续期](adaptive-budget.md) 和 [提示词缓存](prompt-caching.md)。

## 当前验证证据

`2f9c6c1` 的结构重构完成格式检查、workspace 编译、核心库关闭默认 features 编译、release 构建和有限 CLI 人工检查。没有执行测试或真实模型交互，不能据此断言复杂图/协作行为已经完成回归。该轮 release 为 6,318,304 字节，固定工具链和配置见 [结构审查报告](structure-audit-2026-09-10.md)。

本轮 review 修复的范围、接口变化和验证见 [review 修复记录](review-fixes-2026-09-10.md)。[验证入口](validation-2026-09-10.md) 只列当前可执行检查，不再汇总已删除的测试数量。

## 尚未实现或未验证

- 有界并行、不同 Provider 的角色池和更深委托层级尚未实现。
- 增量检查点、Markdown 搜索索引、统一模型/进程执行期限未实现。
- 真实模型的跨服务兼容、规划质量、费用和吞吐收益未做本轮对照。
- 高频 trace 仍逐事件 flush；性能变化需在固定输入下单独测量。

## 历史记录

删除测试前的实现过程与旧验证结果可在 `d35462a:docs/implementation-progress.md` 查看。历史数据不代表当前源码的回归结果，也不证明现在存在同名测试入口。

旧性能报告的源码提交、测量对象及可复现限制统一见 [基准档案说明](benchmarks/README.md)。
