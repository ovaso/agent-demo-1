//! Agent 的长期记忆模型与存储后端。

mod markdown;
mod selection;
pub use selection::{MemorySearchLimits, MemorySelection};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

impl Memory {
    pub fn new(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            tags: Vec::new(),
            source: None,
        }
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
    pub(crate) fn with_source(mut self, source: String) -> Self {
        self.source = Some(source);
        self
    }
    pub(crate) fn bytes(&self) -> usize {
        self.id
            .len()
            .saturating_add(self.content.len())
            .saturating_add(self.source.as_ref().map_or(0, String::len))
            .saturating_add(self.tags.iter().map(String::len).sum::<usize>())
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
    /// Backends should override this to also bound retrieval I/O and allocations.
    fn search_bounded(
        &self,
        query: &str,
        limits: MemorySearchLimits,
    ) -> Result<MemorySelection, MemoryStoreError> {
        if limits.max_results == 0 {
            return Ok(MemorySelection::default());
        }
        Ok(limits.select(self.search(query)?))
    }
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
