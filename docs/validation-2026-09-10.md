# 全局验证记录：2026-09-10

> 历史记录：本文对应删除测试之前的版本，所述测试/基准源码及部分模块路径已变更，不能作为当前验证入口。最新结构与验证见 [结构审查与重构](structure-audit-2026-09-10.md)。

针对 `1482042` 已交付的自适应运行时，完成全量回归与 feature 组合验证，并新增 5 项 CLI 端到端测试。最终所有检查通过，本轮未发现需要修改产品代码的回归问题。

环境：`rustc 1.96.1 (31fca3adb 2026-06-26)`，`aarch64-apple-darwin`。结果仅代表该主机与工具链；不同配置间有重复用例，不能把下表各行相加作为独立测试数量。

## 验证矩阵

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过，无警告 |
| `cargo test --workspace` | 183 项通过，0 失败、0 忽略 |
| `cargo test -p agent-core --no-default-features` | 49 项通过 |
| `cargo clippy -p agent-core --all-targets --no-default-features -- -D warnings` | 通过，无警告 |
| `cargo test -p agent-app --no-default-features --quiet` | 49 项通过，含新增 CLI 测试 |
| `cargo test -p agent-render-cmd --no-default-features` | 62 项通过 |
| `cargo test -p agent-render-cmd --no-default-features --features syntax-highlighting` | 68 项通过 |
| `cargo test -p agent-render-cmd --no-default-features --features math` | 69 项通过 |
| `cargo test --release -p agent-app --test runtime_cli --locked --quiet` | 5 项通过 |
| `cargo build --release -p agent-app --locked` | 通过 |

workspace 的 183 项由应用单元测试 43 项、CLI 端到端测试 5 项、宏集成测试 1 项、核心库测试 59 项和渲染器测试 75 项组成。默认应用配置同时启用数学渲染与语法高亮；上表单独验证了渲染器的其他三种组合。核心库单独关闭默认 features，避免 workspace 的 SQLite feature 合并掩盖问题。

## 新增端到端场景

测试入口为 [runtime_cli.rs](../agent-app/tests/runtime_cli.rs)，自动包含在 `cargo test --workspace` 中。测试辅助代码位于 `agent-app/tests/support/`，按 HTTP 桩与进程/临时文件管理拆分，不进入生产可执行文件，不新增依赖。

1. 无模型凭据时可以查看状态；错误命令行参数返回失败；`/start` 只保存检查点，另一个 CLI 进程可以读取，模型调用数为零。
2. OpenAI 协议下：`/plan` 提交计划与 Blackboard，`/resume` 保持只规划边界；重启后进入 Graph，A 向 B 请求信息并等待，B 答复后根预算耗尽。再次重启并增加总额度后继续原任务，追加文件一次，执行依赖验收节点，最终由协调者输出结果。
3. Anthropic 协议下执行同一条完整链路，检查独立请求格式和分片工具参数解析。
4. OpenAI 流缺少结束标记时，记录模型失败及已消耗步数，不执行流中已出现的工具调用。重启重试后只写文件一次，最终累计 3 次模型调用、1 次工具调用。
5. Anthropic 流中断时执行相同故障与恢复验证。

协作场景还检查：任务 ID 不变、根用量从 1 到 3 再到 6、答复随 SQLite 检查点保留、等待调用和后续写入结果各出现一次、节点全部成功、工具验收依据正确、子 Agent 内部回答不混入主输出，以及 `--trace-map` 可以读取 actor 轨迹。

上述测试启动真正的 CLI 子进程，通过仅监听 `127.0.0.1` 随机端口的 HTTP 桩提供固定响应。子进程清除继承环境，使用虚拟密钥和独立临时数据库、记忆与追踪文件；未连接真实模型服务。测试包含超时与子进程回收。限制本机端口监听的沙箱需要授权该测试命令；不通过忽略测试来绕过限制。

## 发布体积与范围

使用原 release 配置、相同工具链和命令重新构建，`target/release/agent-app` 为 **6,173,808 字节**，相对 `1482042` **变化 0 字节**。相对运行时实现前的 5,790,864 字节基线增加 382,944 字节，约 6.61%。本次仅增加测试与文档，未测量或宣称运行性能收益。

本次验证包含现有 Loop/Graph 切换、计划版本、委托权限与预算、消息等待与超时、恢复故障注入、SQLite 一致性、工具和终端渲染回归。真实模型的规划质量、线上服务兼容差异、其他操作系统与并行执行不在此次验证范围；当前产品仍为顺序调度，详见 [实现记录](implementation-progress.md)。
