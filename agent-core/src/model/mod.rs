//! 模型服务的抽象接口。

mod usage;
pub use usage::ModelUsage;

use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

use super::{
    context::Message,
    memory::Memory,
    tool::{ToolCall, ToolDefinition},
};

/// 一次发送给模型服务的完整请求。
#[derive(Debug)]
pub struct ModelRequest<'a> {
    messages: Vec<Message>,
    memories: &'a [Memory],
    tools: &'a [ToolDefinition],
}

impl<'a> ModelRequest<'a> {
    pub fn new(
        messages: Vec<Message>,
        memories: &'a [Memory],
        tools: &'a [ToolDefinition],
    ) -> Self {
        Self {
            messages,
            memories,
            tools,
        }
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn memories(&self) -> &[Memory] {
        self.memories
    }

    pub fn tools(&self) -> &[ToolDefinition] {
        self.tools
    }
}

/// 模型的一轮响应。
#[derive(Debug, Clone)]
pub struct ModelResponse {
    text: Option<String>,
    tool_calls: Vec<ToolCall>,
    usage: ModelUsage,
}

impl ModelResponse {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            tool_calls: Vec::new(),
            usage: ModelUsage::default(),
        }
    }

    pub fn tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            text: None,
            tool_calls,
            usage: ModelUsage::default(),
        }
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    pub fn with_optional_text(self, text: Option<String>) -> Self {
        match text {
            Some(text) => self.with_text(text),
            None => self,
        }
    }

    pub fn text_content(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn with_usage(mut self, usage: ModelUsage) -> Self {
        self.usage = usage;
        self
    }

    pub fn usage(&self) -> ModelUsage {
        self.usage
    }

    pub fn into_parts(self) -> (Option<String>, Vec<ToolCall>) {
        (self.text, self.tool_calls)
    }
}

/// 模型服务在 Agent loop 中需要实现的最小能力。
pub trait ModelProvider {
    /// 用于追踪的模型标识；不应包含密钥或请求内容。
    fn model_name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    fn complete(&mut self, request: ModelRequest<'_>) -> Result<ModelResponse, ModelError>;

    /// 流式生成文本；默认实现用于不支持流式协议的 provider。
    fn stream(
        &mut self,
        request: ModelRequest<'_>,
        on_text_delta: &mut dyn FnMut(&str),
    ) -> Result<ModelResponse, ModelError> {
        let response = self.complete(request)?;
        if let Some(text) = response.text_content() {
            on_text_delta(text);
        }
        Ok(response)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelError {
    message: String,
}

impl ModelError {
    pub fn new(message: impl Display) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

impl Display for ModelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ModelError {}
