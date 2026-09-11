//! DeepSeek 模型适配器；直接实现 `agent_core::model::ModelProvider`。
//! 配置通过构造函数和 builder 显式传入，不读取应用环境。
mod provider;
pub use provider::{DeepSeekProvider, ReasoningEffort};

const PROTOCOL: &str = "deepseek-chat-completions";
