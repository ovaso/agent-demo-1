//! Agent 与模型服务之间传递的会话状态。

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::tool::ToolCall;

mod message;
pub use message::{Message, Role};

mod compaction;
mod memory;
pub use compaction::{CompactionInfo, ContextWindow};
mod store;

#[cfg(feature = "sqlite")]
mod sqlite;

pub use memory::MemoryContextStore;
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteContextStore;
pub use store::{ContextStore, ContextStoreError};

pub const DEFAULT_HISTORY_LIMIT: usize = 64;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary: Option<Message>,
    history_limit: usize,
    #[serde(default)]
    generation: u64,
    #[serde(default)]
    defer_trimming: bool,
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
            summary: None,
            history_limit,
            generation: 0,
            defer_trimming: false,
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

    pub fn push_observation(&mut self, content: impl Into<String>) {
        self.push(Message::observation(content));
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

    pub(crate) fn defer_trimming(&mut self, deferred: bool) {
        self.defer_trimming = deferred;
        if !deferred {
            self.trim_history();
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub fn history_limit(&self) -> usize {
        self.history_limit
    }

    /// 遍历完整模型上下文，system prompt 始终位于第一条。
    pub fn messages(&self) -> impl Iterator<Item = &Message> {
        self.system
            .iter()
            .chain(self.summary.iter())
            .chain(self.history.iter())
    }

    pub fn history(&self) -> impl Iterator<Item = &Message> {
        self.summary.iter().chain(self.history.iter())
    }

    /// 生成独立持有的消息列表，可用于模型请求、队列或缓存。
    pub fn snapshot(&self) -> Vec<Message> {
        self.messages().cloned().collect()
    }

    pub fn last(&self) -> Option<&Message> {
        self.history
            .back()
            .or(self.summary.as_ref())
            .or(self.system.as_ref())
    }

    pub fn len(&self) -> usize {
        self.history.len()
            + usize::from(self.system.is_some())
            + usize::from(self.summary.is_some())
    }

    pub fn is_empty(&self) -> bool {
        self.system.is_none() && self.summary.is_none() && self.history.is_empty()
    }

    /// 清除 user、assistant 与 tool 消息，但保留 system prompt。
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.summary = None;
        self.generation = self.generation.wrapping_add(1);
    }

    fn trim_history(&mut self) {
        if self.defer_trimming {
            return;
        }
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
            self.generation = self.generation.wrapping_add(1);
        }
    }
}
