use super::{ModelContinuation, ModelUsage};
use crate::tool::ToolCall;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    Complete,
    Length,
    Refused,
    Other(String),
}

impl StopReason {
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Complete => "正常结束",
            Self::Length => "模型输出达到长度上限",
            Self::Refused => "模型服务拒绝继续",
            Self::Other(_) => "模型服务返回未完成状态",
        }
    }
}

/// 模型的一轮响应。
#[derive(Debug, Clone)]
pub struct ModelResponse {
    text: Option<String>,
    reasoning: Option<String>,
    tool_calls: Vec<ToolCall>,
    usage: ModelUsage,
    stop_reason: StopReason,
    continuation: Option<ModelContinuation>,
    response_model: Option<String>,
    request_bytes: Option<usize>,
}

impl ModelResponse {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            reasoning: None,
            tool_calls: Vec::new(),
            usage: ModelUsage::default(),
            stop_reason: StopReason::Complete,
            continuation: None,
            response_model: None,
            request_bytes: None,
        }
    }

    pub fn tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            text: None,
            reasoning: None,
            tool_calls,
            usage: ModelUsage::default(),
            stop_reason: StopReason::Complete,
            continuation: None,
            response_model: None,
            request_bytes: None,
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

    /// 可展示的思考内容，与答案和厂商续接数据分别处理。
    pub fn with_reasoning(mut self, reasoning: Option<String>) -> Self {
        self.reasoning = reasoning;
        self
    }

    pub fn reasoning_content(&self) -> Option<&str> {
        self.reasoning.as_deref()
    }

    pub fn with_usage(mut self, usage: ModelUsage) -> Self {
        self.usage = usage;
        self
    }

    pub fn usage(&self) -> ModelUsage {
        self.usage
    }

    pub fn with_stop_reason(mut self, reason: StopReason) -> Self {
        self.stop_reason = reason;
        self
    }

    pub fn stop_reason(&self) -> &StopReason {
        &self.stop_reason
    }

    pub fn with_continuation(mut self, continuation: Option<ModelContinuation>) -> Self {
        self.continuation = continuation;
        self
    }
    pub fn bind_continuation(&mut self, binding: String) {
        if let Some(continuation) = &mut self.continuation {
            continuation.bind(binding);
        }
    }
    pub fn with_response_model(mut self, model: Option<String>) -> Self {
        self.response_model = model;
        self
    }
    pub fn with_request_bytes(mut self, bytes: usize) -> Self {
        self.request_bytes = Some(bytes);
        self
    }
    pub fn request_bytes(&self) -> Option<usize> {
        self.request_bytes
    }
    pub fn response_model(&self) -> Option<&str> {
        self.response_model.as_deref()
    }
    pub fn into_reply_parts(self) -> (Option<String>, Vec<ToolCall>, Option<ModelContinuation>) {
        (self.text, self.tool_calls, self.continuation)
    }

    pub fn into_parts(self) -> (Option<String>, Vec<ToolCall>) {
        (self.text, self.tool_calls)
    }
}
