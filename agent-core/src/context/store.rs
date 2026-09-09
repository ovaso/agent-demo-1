use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

use super::Context;

/// 会话上下文的可替换存储后端。
///
/// Context 代表内存中的领域数据；本 trait 只处理加载、保存与删除，
/// 因而内存、SQLite、Redis 等实现可以自由替换。
pub trait ContextStore {
    fn load(&self, session_id: &str) -> Result<Option<Context>, ContextStoreError>;
    fn save(&mut self, session_id: &str, context: &Context) -> Result<(), ContextStoreError>;
    fn delete(&mut self, session_id: &str) -> Result<bool, ContextStoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextStoreError {
    EmptySessionId,
    Storage(String),
    Serialization(String),
    Plugin(String),
}

impl ContextStoreError {
    #[cfg(feature = "sqlite")]
    pub(crate) fn storage(error: impl Display) -> Self {
        Self::Storage(error.to_string())
    }

    #[cfg(feature = "sqlite")]
    pub(crate) fn serialization(error: impl Display) -> Self {
        Self::Serialization(error.to_string())
    }
}

impl Display for ContextStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySessionId => formatter.write_str("会话 ID 不能为空"),
            Self::Storage(message) => write!(formatter, "上下文存储失败：{message}"),
            Self::Serialization(message) => write!(formatter, "上下文序列化失败：{message}"),
            Self::Plugin(message) => write!(formatter, "上下文插件失败：{message}"),
        }
    }
}

impl Error for ContextStoreError {}

pub(crate) fn validate_session_id(session_id: &str) -> Result<(), ContextStoreError> {
    if session_id.trim().is_empty() {
        Err(ContextStoreError::EmptySessionId)
    } else {
        Ok(())
    }
}
