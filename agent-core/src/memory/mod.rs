//! Agent 的长期记忆模型与存储后端。

mod markdown;
#[cfg(feature = "sqlite")]
mod sqlite;

use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

use serde::{Deserialize, Serialize};

pub use markdown::MarkdownMemoryStore;
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteMemoryStore;

/// 一条可持久化的长期记忆。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memory {
    id: String,
    content: String,
    tags: Vec<String>,
}

impl Memory {
    pub fn new(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            tags: Vec::new(),
        }
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }
}

/// 长期记忆的可替换存储后端。
pub trait MemoryStore {
    fn get(&self, id: &str) -> Result<Option<Memory>, MemoryStoreError>;
    fn save(&mut self, memory: Memory) -> Result<(), MemoryStoreError>;
    fn list(&self) -> Result<Vec<Memory>, MemoryStoreError>;
    fn search(&self, query: &str) -> Result<Vec<Memory>, MemoryStoreError>;
    fn delete(&mut self, id: &str) -> Result<bool, MemoryStoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryStoreError {
    EmptyId,
    Storage(String),
    Serialization(String),
    InvalidMarkdown(String),
}

impl MemoryStoreError {
    pub(crate) fn storage(error: impl Display) -> Self {
        Self::Storage(error.to_string())
    }

    pub(crate) fn serialization(error: impl Display) -> Self {
        Self::Serialization(error.to_string())
    }
}

impl Display for MemoryStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId => formatter.write_str("记忆 ID 不能为空"),
            Self::Storage(message) => write!(formatter, "记忆存储失败：{message}"),
            Self::Serialization(message) => write!(formatter, "记忆序列化失败：{message}"),
            Self::InvalidMarkdown(message) => {
                write!(formatter, "记忆 Markdown 格式无效：{message}")
            }
        }
    }
}

impl Error for MemoryStoreError {}

pub(crate) fn validate_id(id: &str) -> Result<(), MemoryStoreError> {
    if id.trim().is_empty() {
        Err(MemoryStoreError::EmptyId)
    } else {
        Ok(())
    }
}
