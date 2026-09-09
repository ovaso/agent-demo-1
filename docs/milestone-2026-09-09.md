# 阶段总结：CLI 渲染、内置工具与调用链

日期：2026-09-09。以 `e9e64b2` 为本阶段之前的提交点，将工作区积累的功能整理为可独立编译的提交。

## 提交划分

| 提交 | 内容 | 边界理由 |
| --- | --- | --- |
| `e97374c` | 增量 Markdown 渲染、多行输入、可选代码高亮与数学排版 | 渲染库、依赖与 features、CLI 输入输出接入共同组成可运行功能，合并提交 |
| `3c58514` | `write_file`、`run_cmd` 及内置工具模块拆分 | 文件与进程操作属于应用层，通过现有工具注册机制接入 |
| `c10f1c0` | 分层调用树、token 和缓存统计、查看命令 | 通用节点在 core，协议用量解析和终端展示在 app，整条链路一起提交 |
| 本文所在提交 | 开发约定与阶段总结 | 保存维护规则、验证记录和当前能力边界 |

`main.rs` 的交叠改动按功能分别暂存，没有修改最终工作区内容来制造中间版本。
前两个暂存快照均导出到临时目录执行了 `cargo check --workspace --locked`；第三个提交对应已通过完整验证的最终代码。

## 当前能力

- **终端体验**：Enter 发送、Ctrl+J / Alt+Enter 换行；输入区域支持 Unicode 宽度和窗口变化。环境及终端探测保留在 `agent-app`。
- **渲染**：`agent-render-cmd` 使用持久块状态处理增量文本，支持 Markdown、代码面板、可选语法高亮及 LaTeX 数学排版；保留有界预览、窗口变化和原文输出语义。实现细节、feature 开关及来源许可见 [渲染库说明](../agent-render-cmd/README.md)。
- **文件写入**：`write_file` 支持 create（默认）、overwrite、append，创建缺失父目录，原样保存 UTF-8 文本。
- **命令调用**：`run_cmd` 接受 rg、awk、sed、grep 与 JSON 参数数组，不经 shell；同时消费 stdout/stderr，各保留前 64 KiB，明确返回截断标记和退出状态。
- **运行观测**：每次运行有独立 ID，按运行、轮次、模型／工具／存储操作组织调用树，显示耗时、token、缓存和错误状态。会话输入 `/trace`，或在项目根目录执行 `cargo run -p agent-app -- --trace-map [日志路径]`。字段定义见 [调用链说明](tracing.md)。

## 验证结果

本次提交整理时重新执行：

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo test --workspace`：125 项测试通过。
- `cargo test -p agent-core --no-default-features`：15 项测试通过；关闭 SQLite 后，现有 `ContextStoreError::storage/serialization` 有 dead_code 警告。
- `cargo build --release -p agent-app --locked`：通过。
- Git 补丁空白检查：通过。

调用链实现阶段已用本机 HTTP 模拟服务验证“模型请求 → echo 工具 → 模型回复”，以及 `/trace` 和无 API key 的离线查看。模拟用量合计 220 tokens，缓存读取 100 tokens，缓存命中 2/2；这些是固定测试数据，不是远端模型实测消耗。

Rust 1.96.1、`aarch64-apple-darwin`、原 release profile、默认 features 下，最终 `agent-app` 为 **5,790,864 字节**。加入追踪前的工作区基线为 5,756,384 字节，因此追踪增量为 **34,480 字节（约 0.60%）**。这不是整个阶段相对 `e9e64b2` 的体积变化；渲染接入时的历史测量见 [渲染开发记录](../agent-render-cmd/PLAN.md)。本次整理没有重新测量运行耗时或峰值内存，不宣称性能提升。

## 能力边界与本地材料

- 数学渲染不是完整 TeX 系统；不支持、未闭合或超预算的内容保留明确的源码回退。
- `run_cmd` 的命令白名单不是沙箱，也没有执行超时；输出上限只约束保留的输出字节数。
- token 和缓存来自 Provider 报告，缺失即未知；本地节点不把文件系统或 SQLite 缓存当作模型缓存。
- 父节点时间和 token 包含子调用，不能跨层再次求和。JSONL 文件持续追加，查看器超出大小或节点上限时明确报错。
- 根目录 `latex_render_test.md` 与 `test_latex.tex` 作为本地手工样例保留，不纳入本阶段提交；正式自动化夹具已在渲染库内提交。`tool_test/` 输出由 `.gitignore` 排除。
