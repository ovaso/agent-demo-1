# 02 · 自适应运行时当前实现状态

[阅读索引](README.md) · 上一篇：[01](01-adaptive-agent-runtime.md) · 下一篇：[03](03-runtime-cli.md)

文档性质：实现状态与限制；历史验证不代表当前回归结果。

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
| 模型适配 | `agent-vendor-openai/`、`agent-vendor-anthropic/`、`agent-vendor-deepseek/` | 独立实现核心 ModelProvider；核心统一答案/思考展示事件，DeepSeek 独立适配推理档位；应用只负责选择和装配，共享支持位于 `agent-vendor/`。 |

使用方法见 [运行时 CLI](03-runtime-cli.md)、[环境配置](../common/environment.md)、[预算续期](../budget/README.md) 和 [提示词缓存](../context/05-cache-and-input-budget.md)。

## 当前验证证据

当前验证要求以[项目约定](../../AGENTS.md#验证与交付)为准。历史结构重构和 review 的结果见下方 Git 历史记录，不作为当前回归证据。

2026-09-11 提交前，对厂商适配拆分、DeepSeek 接入、思考流展示及文档整理的完整工作区重新完成：

- `cargo fmt --all -- --check`。
- `cargo check --workspace`。
- `cargo check -p agent-core --no-default-features`。
- `cargo build --release -p agent-app --locked`。
- `git diff --check` 和本地 Markdown 链接目标核对，修复文档迁移后失效的入口。

在独立临时目录中，以占位密钥和本机不可用端点分别启动 DeepSeek、OpenAI、Anthropic 配置，人工检查 `/status`、`/help` 和 `/quit`：三者均正常启动、提示当前没有任务、显示帮助并退出。此次仅核对离线装配与 CLI，没有调用真实模型服务，没有新增测试、夹具或基准程序，也没有运行 `cargo test`；不代表模型协议或复杂运行时行为已经完成回归。

体积比较使用相同 rustc 1.96.1、aarch64-apple-darwin、默认 features 和上述 release 命令：从 Git 导出基线 `88e1641` 并构建，主可执行文件为 6,335,856 字节；当前为 6,417,520 字节，增加 81,664 字节（约 1.29%）。本次新增四个本地 vendor crate，第三方依赖、其启用的 features 和 release 配置未变。新增适配与思考展示能力伴随产物增长，未测量运行耗时或峰值内存，不宣称性能收益。

## 尚未实现或未验证

- 有界并行、不同 Provider 的角色池和更深委托层级尚未实现。
- 增量检查点、Markdown 搜索索引、统一模型/进程执行期限未实现。
- 真实模型的跨服务兼容、规划质量、费用和吞吐收益未做本轮对照。
- 高频 trace 仍逐事件 flush；性能变化需在固定输入下单独测量。

## 历史记录

`2f9c6c1` 的结构重构曾完成格式检查、workspace 编译、核心库关闭默认 features 编译、release 构建和有限 CLI 人工检查；当时主可执行文件为 6,318,304 字节，未执行自动化测试或真实模型交互。原报告可用 `git show 88e1641:docs/structure-audit-2026-09-10.md` 查阅；后续 review 与验证记录分别保存在 `88e1641:docs/review-fixes-2026-09-10.md` 和 `88e1641:docs/validation-2026-09-10.md`。这些文件已从当前文档目录移除。

删除测试前的实现过程与旧验证结果可在 `d35462a:docs/implementation-progress.md` 查看。历史数据不代表当前源码的回归结果，也不证明现在存在同名测试入口。

旧性能报告的源码提交、测量对象及可复现限制统一见 [基准档案说明](../benchmarks/README.md)。

厂商实现与应用装配的当前边界见[项目约定](../../AGENTS.md#代码放置与模块边界)和[公共适配支持](../../agent-vendor/README.md)。
