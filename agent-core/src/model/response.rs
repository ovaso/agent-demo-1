use super::ModelUsage;
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
    tool_calls: Vec<ToolCall>,
    usage: ModelUsage,
    stop_reason: StopReason,
}

impl ModelResponse {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            tool_calls: Vec::new(),
            usage: ModelUsage::default(),
            stop_reason: StopReason::Complete,
        }
    }

    pub fn tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            text: None,
            tool_calls,
            usage: ModelUsage::default(),
            stop_reason: StopReason::Complete,
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

    pub fn with_stop_reason(mut self, reason: StopReason) -> Self {
        self.stop_reason = reason;
        self
    }

    pub fn stop_reason(&self) -> &StopReason {
        &self.stop_reason
    }

    pub fn into_parts(self) -> (Option<String>, Vec<ToolCall>) {
        (self.text, self.tool_calls)
    }
}
