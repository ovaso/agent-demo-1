//! 会话消息与角色；不负责历史裁剪或持久化。
use crate::{model::ModelContinuation, tool::ToolCall};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// Agent 会话中的一条消息。
///
/// 枚举让非法状态无法构造：只有工具结果带有工具名，调用方不能误把
/// 不匹配角色的字段组合在一起。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Message {
    System {
        content: String,
    },
    User {
        content: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        rebuildable: bool,
    },
    Assistant {
        content: String,
        #[serde(default)]
        tool_calls: Vec<ToolCall>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        continuation: Option<ModelContinuation>,
    },
    Tool {
        #[serde(default)]
        call_id: String,
        name: String,
        content: String,
    },
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self::System {
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::User {
            content: content.into(),
            rebuildable: false,
        }
    }

    /// Runtime-generated data that can be reconstructed after compaction.
    pub fn observation(content: impl Into<String>) -> Self {
        Self::User {
            content: content.into(),
            rebuildable: true,
        }
    }
    pub fn is_instruction(&self) -> bool {
        matches!(
            self,
            Self::User {
                rebuildable: false,
                ..
            }
        )
    }
    pub fn is_observation(&self) -> bool {
        matches!(
            self,
            Self::User {
                rebuildable: true,
                ..
            }
        )
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::Assistant {
            content: content.into(),
            tool_calls: Vec::new(),
            continuation: None,
        }
    }

    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        Self::Assistant {
            content: content.into(),
            tool_calls,
            continuation: None,
        }
    }

    pub fn assistant_reply(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
        continuation: Option<ModelContinuation>,
    ) -> Self {
        Self::Assistant {
            content: content.into(),
            tool_calls,
            continuation,
        }
    }

    pub fn continuation(&self) -> Option<&ModelContinuation> {
        match self {
            Self::Assistant { continuation, .. } => continuation.as_ref(),
            _ => None,
        }
    }

    pub fn tool(
        call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self::Tool {
            call_id: call_id.into(),
            name: name.into(),
            content: content.into(),
        }
    }

    pub fn role(&self) -> Role {
        match self {
            Self::System { .. } => Role::System,
            Self::User { .. } => Role::User,
            Self::Assistant { .. } => Role::Assistant,
            Self::Tool { .. } => Role::Tool,
        }
    }

    pub fn content(&self) -> &str {
        match self {
            Self::System { content }
            | Self::User { content, .. }
            | Self::Assistant { content, .. }
            | Self::Tool { content, .. } => content,
        }
    }

    pub fn tool_name(&self) -> Option<&str> {
        match self {
            Self::Tool { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn tool_call_id(&self) -> Option<&str> {
        match self {
            Self::Tool { call_id, .. } => Some(call_id),
            _ => None,
        }
    }

    pub fn tool_calls(&self) -> Option<&[ToolCall]> {
        match self {
            Self::Assistant { tool_calls, .. } => Some(tool_calls),
            _ => None,
        }
    }
}
