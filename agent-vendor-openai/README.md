# agent-vendor-openai

独立模型适配库，直接实现 `agent_core::model::ModelProvider`。保留 OpenAI Chat Completions 兼容协议与 reasoning_content 展示/续接兼容能力。DeepSeek 原生 thinking 开关与档位策略由独立 `agent-vendor-deepseek` 负责，不再按 base URL 注入。

无需 `agent-app` 或另一个 vendor crate。依赖核心时关闭其默认 features，单独使用不会启用 SQLite。应用显式传入密钥、模型、端点和策略；本库不读取 `.env`、应用配置或终端。

```rust
use agent_core::model::{ModelError, ModelProvider, ModelRequest, ModelResponse};
use agent_vendor_openai::OpenAiCompatibleProvider;

fn complete(api_key: String, request: ModelRequest<'_>) -> Result<ModelResponse, ModelError> {
    let mut provider = OpenAiCompatibleProvider::new(api_key, "your-model");
    provider.complete(request)
}
```

长期运行时应持有并复用一个 Provider 实例。需要自定义超时、代理、证书或连接策略时，用 `with_client(client, api_key, model)` 注入已配置的 `reqwest::blocking::Client`。默认构造保留原 HTTP 行为；`stream` 逐增量回调，不先收齐响应。

内部 `provider/` 按 `request.rs`、`response.rs`、`stream.rs`、`http.rs`、`usage.rs`、`reasoning.rs` 和 `stop_reason.rs` 分工。协议标识保持原值，因此 crate 路径变化不改变保存的续接数据。

依赖方向见[项目约定](../AGENTS.md#代码放置与模块边界)，当前验证见[实现状态](../docs/runtime/02-implementation-progress.md#当前验证证据)。
