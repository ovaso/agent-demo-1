# 自适应运行时实现记录

按功能独立提交，设计目标见 [adaptive-agent-runtime.md](adaptive-agent-runtime.md)。本记录区分已交付能力与后续工作，不将设计草案视为已实现。

## 构建基线

- 工具链：`rustc 1.96.1 (31fca3adb 2026-06-26)`。
- 命令：`cargo build --release -p agent-app --locked`，默认 features，当前主机 target。
- 基线可执行文件：`target/release/agent-app`，5,790,864 字节。
- 未建立运行性能基准，不宣称耗时或内存收益。

## 交付顺序

1. 协议完整的上下文裁剪。
2. 可恢复 Loop、统一模型步数与逐工具检查点。
3. SQLite 运行存储与 CLI 恢复、状态和预算入口。
4. Plan、Blackboard 与受限只规划模式。
5. 模型路由、Graph 与途中模式切换。
6. 子 Agent 委托与共享预算。
7. 子 Agent 消息协作、等待和恢复。
8. 基于测量决定有界并发的实现与默认配置。

## 已交付

- 设计与领域术语入库；尚未修改运行代码。
