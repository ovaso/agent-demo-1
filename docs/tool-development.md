# 使用工具宏开发 Agent Tool

`#[tool]` 现在同时生成 `Tool` 实现、保留手动工厂函数，并将工厂加入可自动发现的工具组。应用只在启动时按组装配一次；新增同组工具不再维护逐项注册列表。

此注册方式用于普通业务工具与 debug 工具。`agent-core` 的 `runtime_*` 规划、调度、协作、预算和消息能力继续走核心专用的注册及执行路径，不接入通用宏注册表；它们需要在任务检查点和权限边界内操作实时状态。测试内的工具替身也不属于本次普通工具迁移范围。

## 普通工具

在独立 workspace crate `agent-app-tool/src` 下已纳入编译的模块中定义：

```rust
use agent_core::tool;

/// 返回指定文本，可选择重复次数。
#[tool(created_at = 1789031513, version = "v1.0.0-20260910", read_only, output = "text")]
fn repeat_text(
    #[arg(description = "需要返回的文本")] text: String,
    count: Option<usize>,
) -> String {
    text.repeat(count.unwrap_or(1).min(8))
}
```

`group` 省略时为 `"default"`。应用会话通过 `agent_app_tool::register(&mut tools)?` 装配（内部启用默认组），因此这个函数自动出现在模型可用工具中。新增 `.rs` 文件仍需按 Rust 规则在父模块添加 `mod 文件名;`；宏不会扫描磁盘上未参与编译的源文件。

`agent-app-tool` 中的 `write_file`、`read_file`、`list_directory`、`search_files`、`run_cmd`、`run_check` 和 `session_finish` 均使用宏注册；启动入口不再逐个构造工具。各工具保留原有名称、参数说明、可选性、只读标记与返回格式，测试用迁移前捕获的完整定义快照验证检查点兼容性。

其他应用复用这个 toolset 时，在装配处调用一次：

```rust
let mut tools = agent_core::tool::Registry::new();
agent_app_tool::register(&mut tools)?;
```

依赖 crate 中的工具同样可被发现，但该 crate 必须实际链接进最终应用；仅把依赖写在 Cargo.toml 中并不足够。`agent_app_tool::register` 和 `agent_tool_debug::register` 分别是普通与 debug toolset 的链接和初始化入口。重复启用同一组会报告重名错误，不会覆盖已有工具；不要同时手动注册某个工具并启用包含它的组。

## 调试工具与状态注入

调试工具统一标注 `group = "debug"`。应用仅在 `--mode=debug` 时调用 debug crate 的初始化入口；链接进二进制的调试工厂元数据不会自行创建工具或加入模型请求。

目前 debug 组包含 `debug_show_config`（启动配置查看）和 `echo`（工具调用链路回显）。`echo` 已从 `agent-app/src/tools` 迁入 `agent-tool-debug/src/echo.rs`，保留原有名称、参数、权限和 JSON 字符串输出；普通模式不再注册。若旧任务的检查点已经包含 `echo` 授权，恢复时应以 `--mode=debug` 启动，以满足原有工具定义校验。

按用途归类：文件读写、目录列举、搜索、命令执行、`run_check` 和结束会话属于实际任务能力；核心库的 `runtime_agents`、`runtime_result`、共享记录与收件箱工具属于正常协作协议。它们不因只读或检查性质而归入 debug。核心库测试内的 `EchoTool`、`Counter`、`Probe` 等是仅在测试时编译的替身，保留在原测试模块，避免让核心库反向依赖 debug crate。

现有配置工具位于 `agent-tool-debug/src/show_config.rs`：

```rust
use agent_core::tool;
use super::DebugContext;

/// 显示启动时生效的配置，不包含 API 密钥。
#[tool(created_at = 1789028730, version = "v1.0.0-20260910", group = "debug", read_only, output = "text")]
fn debug_show_config(#[context] context: &DebugContext) -> &str {
    &context.config_snapshot
}
```

`#[context]` 参数来自应用，不会出现在工具参数定义中，模型也不能传入它。每个函数至多注入一个 `&State`；应用通过 `Arc<State>` 共享状态，调用工具时只借用它。State 必须是 `Send + Sync + 'static`，状态中的可变数据自行使用适当的同步机制。

```rust
tools.register_group_with_context("debug", std::sync::Arc::new(context))?;
```

一个组可混合有状态和无状态工具；有状态工具须接受同一种 State。新 debug 工具只需在同一 crate 的编译模块里标注宏，无需修改 `register` 列表；可在 `DebugContext` 中扩展所需状态。缺少上下文或类型不匹配会在注册时报告具体工具与期望类型。不同 Registry 可以注入不同状态，不使用全局配置单例。

## 首次加入时间、名称与参考版本

`ToolDefinition` 保留原字段，并增加 `created_at: u64` 和 `version: String`；序列化字段开头为 `created_at`、`name`、`version`。宏要求显式提供正整数 `created_at` 和非空字符串 `version`，不读取当前时间、Git 或文件时间，也不按构建时间自动生成元数据。名称继续支持省略时取函数名及 `name = "..."` 覆盖。

注册工厂、Registry 输出及最终模型工具列表都按 `(created_at, name)` 升序，版本完全不参与比较。新增工具使用真实的首次加入时间，晚于现有工具时，即使名字字母顺序靠前也排在后面；同一时间按名称打破并列。不要在文件移动、重构、重新编译或版本更新时修改已有时间戳。示例里的时间戳是固定示例，新工具应填写自身首次加入时间。

既有工具按 Git 首次引入的提交时间回填，提交时间是可追溯的历史近似；`debug_show_config` 使用首次实现时的文件创建记录。初始参考版本统一为 `v1.0.0-20260910`。时间来源：

| 工具 | created_at（Unix 秒） | 来源 |
| --- | --- | --- |
| `echo`、`session_finish` | 1788784117 | `8a185b0`，2026-09-07 |
| `run_cmd`、`write_file` | 1788951445 | `3c58514`，2026-09-09 |
| 文件/目录读取、搜索、初始规划与共享记录能力 | 1789006513 | `8588a0b`，2026-09-10 |
| `run_check`、路由和图节点控制能力 | 1789008818 | `8637e48`，2026-09-10 |
| Agent 委托、列表、预算、取消和结果能力 | 1789010818 | `2b73c32`，2026-09-10 |
| 核心消息能力 | 1789013643 | `1482042`，2026-09-10 |
| `debug_show_config` | 1789028730 | 本次源码文件创建记录，2026-09-10 |

核心能力补充元数据时使用 `ToolDefinition::with_metadata`，不改成宏注册。旧 JSON 检查点缺失的时间和版本默认为 `0`、空字符串，恢复时只为原已授权工具补齐当前元数据；不会自动授予新增工具。只有名称、描述、参数或只读权限等执行契约改变才触发原有不兼容检查，参考版本变化不会单独阻止恢复。旧手写测试替身和兼容构造器继续使用这些缺省值，生产工具已全部填写对应的固定值。

时间戳和版本保存在本地定义和检查点中，provider 工具 schema 及模型可见的运行状态定义不附加这些参考字段，因此仅修改版本不会改变工具 schema 文本。稳定顺序只消除顺序引起的变化；若修改已有名称、描述、参数、权限，或倒填时间，仍可能改变缓存前缀。本次没有测量远端缓存命中率。

## 宏选项

| 写法 | 行为 |
| --- | --- |
| `created_at = 1789031513` | 必填，首次加入源码时固定的 Unix 秒时间戳；迁移或改版本不修改它 |
| `version = "v1.0.0-20260910"` | 必填，非空参考版本，可含构建日期，不参与排序 |
| `name = "tool_name"` | 自定义工具名，默认函数名 |
| `description = "说明"` | 自定义说明；省略时优先使用函数的 `///` 文档 |
| `group = "debug"` | 自定义注册组，默认 `default` |
| `read_only` | 声明纯读取能力，使工具可在 Plan Mode 中调用；宏不会分析函数副作用 |
| `output = "json"` | 默认输出方式，序列化返回值；字符串保留 JSON 引号与转义，兼容原行为 |
| `output = "text"` | 返回 `String`、`&str` 等可转为 String 的文本，保留真实换行 |
| `output = "tool"` | 原样返回 `ToolOutput`，保留成功/失败和结束会话语义 |
| `finish_session` | 将 JSON 或文本返回值标记为结束会话；默认 JSON 模式保留旧版字符串去引号行为 |

`Result<T, E>` 自动传播错误，E 需支持 Display；T 按选定 output 处理。`finish_session` 不能与 `read_only` 或 `output = "tool"` 同用；后者直接用 `ToolOutput::finish_session` 表达。

```rust
#[agent_core::tool(created_at = 1789031513, version = "v1.0.0-20260910", output = "tool")]
fn check_result() -> Result<agent_core::tool::ToolOutput, std::io::Error> {
    Ok(agent_core::tool::ToolOutput::text("检查未通过").with_success(false))
}
```

普通输入支持拥有所有权且可被 Serde 反序列化的类型，如 String、整数、布尔值、Vec 和结构体。直接写 `Option<T>` 的参数可以省略，得到 None，其余参数必须提供。每个参数可通过 `#[arg(description = "说明")]` 描述。输入按目标类型尝试 JSON 解析，再尝试作为字符串解码，因此 String 参数可接受原样的 `123`、`true` 或多行文本。显式 JSON 字符串会解码引号和转义；Option 参数传入 JSON `null` 表示 None。

需要保留 provider 已解码的原始文本时，使用 `&str` 或 `Option<&str>`：宏直接借用 `Arguments` 中的文本，不再做 JSON 解码或复制。带引号的文本、字面的 `null`、反斜线和换行均原样保留；`Option<&str>` 仅在参数缺失时为 None，显式 `null` 文本为 `Some("null")`。文件内容、路径和命令参数等普通工具采用此方式，避免改变用户输入。其他借用参数类型仍不支持。

仍仅支持无泛型的安全同步自由函数；不支持 async、self 方法、可变或显式生命周期的 context 引用、参数解构。错误选项、重复选项与不支持的签名会在编译时给出错误。已有 `function_name_tool()` 手动工厂仍可用；含 context 时工厂接收 `Arc<State>`。

## 实现边界与验证

宏 crate 的 `config.rs`、`parameters.rs`、`expand.rs` 分别负责选项、签名和代码生成。参数解码和 JSON 输出共享实现放在 `agent-core/src/tool/invocation.rs`，自动收集与按组装配放在 `agent-core/src/tool/automatic.rs`。注册时按 `(created_at, name)` 排序，先暂存并验证整组，再加入 Registry；重名、缺少上下文等失败不会留下部分注册结果。

跨模块和 crate 的发现采用 [inventory 0.3.24](https://docs.rs/inventory/0.3.24/inventory/)，标准库与原有依赖未提供这一能力。关闭默认 features；当前 macOS target 下它没有普通或构建传递依赖，wasm 条件依赖为 rustversion（已在原锁文件中）。不新增异步运行时，不改变 SQLite 的可选 feature，也不修改 release 配置。inventory 的初始化仅收集静态工厂元数据，应用启用某组时才创建工具与绑定状态。

测试覆盖旧工厂兼容、普通组自动发现、跨模块状态共享与跨实例隔离、参数可选性和错误、输入状态不可伪造、输出语义、失败时原子注册、条件编译和非法宏签名；CLI 测试覆盖实际跨 crate debug 注册、普通模式不可见、配置快照与两类 provider。

宏扩展阶段验证全部通过：

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`（233 项）
- `cargo test -p agent-core --no-default-features`（67 项）
- `cargo test -p agent-tool-debug --no-default-features`（1 项）
- `cargo test --release -p agent-app --test runtime_cli --test tool_registration --test tool_macro`（26 项，验证优化构建下的自动发现和模式隔离）
- `cargo build --release -p agent-app --locked`

以本轮开始时已经添加 debug 工具的版本为基线，在相同 `rustc 1.96.1`、`aarch64-apple-darwin`、默认 features 和上述 release 构建命令下，主可执行文件从 6,193,168 增至 6,196,016 字节，增加 2,848 字节（约 0.05%）。这是宏扩展与自动注册整体的产物变化，不单独归因于某一依赖；未测量运行耗时或分配次数变化。

随后迁移 `echo`：通过格式检查、workspace Clippy、workspace 全量测试（235 项）及 release debug CLI 测试（6 项）。测试覆盖原工具定义和输出兼容、普通模式禁用 debug 工具、两种 provider 下的 debug 调用，以及普通模式保留正常任务和协作工具。同一工具链、target、默认 features 和 `cargo build --release -p agent-app --locked` 命令下，主可执行文件从 6,196,016 增至 6,196,816 字节，增加 800 字节（约 0.013%）。没有新增依赖，未测量运行性能变化。

普通业务工具宏迁移：六个手写工具实现已改为 `#[tool]`，与原有 `session_finish` 一起按默认组注册；核心能力类的注册与执行代码未改。新增原始文本借用测试、完整工具定义兼容快照和借用参数诊断测试。通过格式检查、workspace Clippy、workspace 全量测试（238 项）、核心库无默认 feature 测试（67 项），以及 release CLI 和宏注册测试（27 项）。同一工具链、target、默认 features 和 `cargo build --release -p agent-app --locked` 命令下，产物从 6,196,816 增至 6,196,928 字节，增加 112 字节（约 0.002%）。没有新增依赖，未做运行性能对比。

普通 toolset 独立为 `agent-app-tool`：包含七个工具、共享命令输出处理与原有测试；`agent-app` 通过 `agent_app_tool::register` 装配。直接依赖只有关闭默认 features 的 `agent-core` 和现有 `serde_json`，独立依赖树不包含 SQLite、HTTP provider 或终端渲染。自动注册和 Registry 输出维持名称顺序；模型请求合并核心能力并完成权限过滤后也统一按名称排序。新增测试验证 toolset 注册顺序无关、两个 provider 的跨进程序列一致，并在所有 CLI mock 请求中检查最终工具数组有序。

本阶段通过格式检查、workspace Clippy、workspace 全量测试（240 项）、核心库无默认 feature 测试（67 项）、独立 toolset 无默认 feature 测试（19 项），以及 release CLI 和 toolset 集成测试（24 项）。同一 `rustc 1.96.1`、`aarch64-apple-darwin`、默认 features 和 `cargo build --release -p agent-app --locked` 命令下，产物从 6,196,928 增至 6,217,904 字节，增加 20,976 字节（约 0.34%）。没有新增第三方依赖；未测运行耗时和模型服务缓存命中率。稳定排序避免顺序波动，不减少工具定义本身的 token 数。

时间戳与版本元数据：宏必填字段、创建时间/名称排序、版本不参与排序、老 JSON 反序列化与检查点元数据补齐、权限契约不放宽、模型可见定义不携带参考版本等测试均通过。最终通过格式检查、workspace Clippy、workspace 全量测试（245 项）、核心库无默认 feature 测试（70 项）、独立 toolset 测试（19 项）和 release CLI/注册/toolset 测试（30 项）。同一 `rustc 1.96.1`、`aarch64-apple-darwin`、默认 features 和 `cargo build --release -p agent-app --locked` 命令下，主可执行文件从 6,217,904 增至 6,218,784 字节，增加 880 字节（约 0.014%）。没有新增依赖；未做运行性能对比或远端缓存命中率测试。
