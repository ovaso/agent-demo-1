//! 第三方模型服务适配器。

mod anthropic;
mod openai;

pub use anthropic::AnthropicProvider;
pub use openai::OpenAiCompatibleProvider;
