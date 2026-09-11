//! 模型服务的抽象接口。

mod continuation;
mod input;
mod response;
pub(crate) use input::encoded_size;
pub use input::{InputDiagnostics, InputDigest};
mod usage;
pub use continuation::ModelContinuation;
pub use response::{ModelResponse, StopReason};
pub use usage::ModelUsage;

use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

use super::{context::Message, memory::Memory, tool::ToolDefinition};

/// 一次发送给模型服务的完整请求。
#[derive(Debug)]
pub struct ModelRequest<'a> {
    messages: Vec<Message>,
    memories: &'a [Memory],
    tools: &'a [ToolDefinition],
    max_input_bytes: usize,
    max_output_tokens: Option<u64>,
    diagnostics: Option<InputDiagnostics>,
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
            max_input_bytes: 1024 * 1024,
            max_output_tokens: None,
            diagnostics: None,
        }
    }

    pub(crate) fn with_diagnostics(mut self, diagnostics: InputDiagnostics) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }
    pub fn with_max_output_tokens(mut self, limit: Option<u64>) -> Self {
        self.max_output_tokens = limit;
        self
    }
    pub fn max_output_tokens(&self) -> Option<u64> {
        self.max_output_tokens
    }

    pub fn with_max_input_bytes(mut self, limit: usize) -> Self {
        self.max_input_bytes = limit;
        self
    }
    pub fn max_input_bytes(&self) -> usize {
        self.max_input_bytes
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

/// 厂商公开返回的可展示增量；不包含签名、加密块等 opaque 续接信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelStreamEvent<'a> {
    TextDelta(&'a str),
    ReasoningDelta(&'a str),
}

/// 模型服务在 Agent loop 中需要实现的最小能力，也是所有适配器的公共 contract。
///
/// 实现方负责鉴权、传输、厂商请求/响应转换和增量事件解析；核心只消费这些
/// 通用模型类型。实现可位于任意 crate，无需依赖某个 vendor 支持库或 HTTP。
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

    /// 通用展示事件。旧 provider 的文本流仍然增量转发；支持思考流的实现应重写此方法。
    fn stream_events(
        &mut self,
        request: ModelRequest<'_>,
        on_event: &mut dyn FnMut(ModelStreamEvent<'_>),
    ) -> Result<ModelResponse, ModelError> {
        self.stream(request, &mut |text| {
            on_event(ModelStreamEvent::TextDelta(text))
        })
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
