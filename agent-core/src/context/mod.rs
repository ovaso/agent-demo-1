//! Agent 与模型服务之间传递的会话状态。

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::{model::ModelContinuation, tool::ToolCall};

mod memory;
#[cfg(test)]
mod protocol_tests;
mod store;

#[cfg(feature = "sqlite")]
mod sqlite;

pub use memory::MemoryContextStore;
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteContextStore;
pub use store::{ContextStore, ContextStoreError};

pub const DEFAULT_HISTORY_LIMIT: usize = 64;

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
        }
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
            | Self::User { content }
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

/// 单次 Agent 运行的有界会话历史。
///
/// System prompt 独立保存，永远不会受历史上限影响；上限只作用于
/// user、assistant 与 tool 消息。工具请求及其连续结果作为整体裁剪；
/// 位于末尾的工具批次暂时允许超过消息上限，直到后续消息到来。
/// 调用方仍需限制单批工具数量和结果字节数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    system: Option<Message>,
    history: VecDeque<Message>,
    history_limit: usize,
}

impl Default for Context {
    fn default() -> Self {
        Self::with_history_limit(DEFAULT_HISTORY_LIMIT)
    }
}

impl Context {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_history_limit(history_limit: usize) -> Self {
        Self {
            system: None,
            history: VecDeque::new(),
            history_limit,
        }
    }

    pub fn with_system(system_prompt: impl Into<String>) -> Self {
        let mut context = Self::new();
        context.set_system_prompt(system_prompt);
        context
    }

    pub fn set_system_prompt(&mut self, content: impl Into<String>) {
        self.system = Some(Message::system(content));
    }

    pub fn clear_system_prompt(&mut self) {
        self.system = None;
    }

    /// 追加消息；system 消息会替换已有的 system prompt。
    pub fn push(&mut self, message: Message) {
        match message {
            Message::System { .. } => self.system = Some(message),
            message => {
                self.history.push_back(message);
                self.trim_history();
            }
        }
    }

    pub fn push_user(&mut self, content: impl Into<String>) {
        self.push(Message::user(content));
    }

    pub fn push_assistant(&mut self, content: impl Into<String>) {
        self.push(Message::assistant(content));
    }

    pub fn push_assistant_with_tool_calls(
        &mut self,
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
    ) {
        self.push(Message::assistant_with_tool_calls(content, tool_calls));
    }

    pub fn push_tool(
        &mut self,
        call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) {
        self.push(Message::tool(call_id, name, content));
    }

    /// 更新上限，并按完整工具批次丢弃最旧历史，保留末尾批次。
    pub fn set_history_limit(&mut self, history_limit: usize) {
        self.history_limit = history_limit;
        self.trim_history();
    }

    pub fn history_limit(&self) -> usize {
        self.history_limit
    }

    /// 遍历完整模型上下文，system prompt 始终位于第一条。
    pub fn messages(&self) -> impl Iterator<Item = &Message> {
        self.system.iter().chain(self.history.iter())
    }

    pub fn history(&self) -> impl Iterator<Item = &Message> {
        self.history.iter()
    }

    /// 生成独立持有的消息列表，可用于模型请求、队列或缓存。
    pub fn snapshot(&self) -> Vec<Message> {
        self.messages().cloned().collect()
    }

    pub fn last(&self) -> Option<&Message> {
        self.history.back().or(self.system.as_ref())
    }

    pub fn len(&self) -> usize {
        self.history.len() + usize::from(self.system.is_some())
    }

    pub fn is_empty(&self) -> bool {
        self.system.is_none() && self.history.is_empty()
    }

    /// 清除 user、assistant 与 tool 消息，但保留 system prompt。
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    fn trim_history(&mut self) {
        while self.history.len() > self.history_limit {
            let remove = match self.history.front() {
                Some(Message::Assistant { tool_calls, .. }) if !tool_calls.is_empty() => {
                    let end = 1 + self
                        .history
                        .iter()
                        .skip(1)
                        .take_while(|message| matches!(message, Message::Tool { .. }))
                        .count();
                    if end == self.history.len() {
                        break;
                    }
                    end
                }
                _ => 1,
            };
            self.history.drain(..remove);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_system_prompt_and_latest_history() {
        let mut context = Context::with_history_limit(2);
        context.set_system_prompt("Be concise.");
        context.push_user("first");
        context.push_assistant("second");
        context.push_tool("call-1", "echo", "third");

        let messages = context.snapshot();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0], Message::system("Be concise."));
        assert_eq!(messages[1], Message::assistant("second"));
        assert_eq!(messages[2], Message::tool("call-1", "echo", "third"));
    }

    #[test]
    fn system_messages_replace_the_existing_prompt() {
        let mut context = Context::with_system("old prompt");
        context.push(Message::system("new prompt"));

        assert_eq!(context.snapshot(), vec![Message::system("new prompt")]);
    }

    #[test]
    fn clearing_history_preserves_the_system_prompt() {
        let mut context = Context::with_system("You are helpful.");
        context.push_user("hello");
        context.clear_history();

        assert_eq!(context.len(), 1);
        assert_eq!(context.last().unwrap().role(), Role::System);
    }
}
