# agent-app-tool

Agent 的普通业务 toolset，是独立的 workspace library crate。`agent-app` 负责配置、模型和会话装配，本 crate 负责 Agent 可执行的普通业务工具。

```rust
let mut tools = agent_core::tool::Registry::new();
agent_app_tool::register(&mut tools)?;
let definitions = tools.definitions(); // 按 created_at、name 升序；version 不参与排序
```

## 当前 toolset

| 工具 | 用途 | 只读 |
| --- | --- | --- |
| `list_directory` | 有界目录列举 | 是 |
| `read_file` | 有界文件读取 | 是 |
| `run_check` | 固定 Rust 验证命令 | 否 |
| `run_cmd` | 启动白名单命令 | 否 |
| `search_files` | 固定文本搜索 | 是 |
| `session_finish` | 明确结束会话 | 否 |
| `write_file` | 新建、覆盖或追加文本 | 否 |

debug 工具由 `agent-tool-debug` 提供；核心 `runtime_*` 能力保留专用注册及执行路径。

## 扩展

在 `src/` 新增职责明确的模块，并在 `src/lib.rs` 添加 `mod` 声明。普通工具通过 `#[agent_core::tool(...)]` 加入默认组，无需修改逐项注册列表；无需公开具体实现类型。文本参数可使用 `&str` / `Option<&str>` 保留 provider 已解码的原文。

```rust
/// 返回指定文本。
#[agent_core::tool(created_at = 1789031513, version = "v1.0.0-20260910", read_only, output = "text")]
fn example(text: &str) -> &str {
    text
}
```

宏选项与状态注入见[工具开发文档](../docs/tools/README.md)。`register` 收集所有实际链接的默认组工具，因此宿主也可通过自身已编译模块添加默认组工具。重复工具名会报错，并保持原注册表不变。

只直接依赖关闭默认 features 的 `agent-core` 与现有 `serde_json`；不绑定 SQLite、模型服务、终端渲染或 CLI。单独验证：

```sh
cargo check -p agent-app-tool --no-default-features
```

## 顺序稳定性

每个宏必须声明固定的首次加入时间戳 `created_at` 和参考 `version`。自动发现与 Registry 输出均按 `(created_at, name)` 升序，版本不参与排序；普通与 debug toolset 的注册先后不会影响工具定义序列。运行时在合并普通工具与核心能力、完成权限过滤后，对最终发给模型的列表再次按 `(created_at, name)` 排序。

新增工具应使用晚于现有工具的实际首次加入时间；已有时间不随文件迁移、重新构建或版本变化而更新。这样晚加入的工具即使名字靠前也不会插入旧工具之间。排序使同一工具集合的定义序列稳定，避免仅因顺序变化影响提示缓存复用。它不减少定义本身的输入 token 数，也不保证模型服务的缓存命中率。
