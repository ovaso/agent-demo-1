use crate::{agent::AgentError, memory::MemoryStoreError, model::ModelError, trace::TraceError};
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    io,
};

/// 运行失败的分类。具体原因通过 Error::source 保留；错误类别不意味着可自动重试。
#[derive(Debug)]
pub enum RuntimeError {
    Invalid(String),
    NotFound(String),
    Conflict,
    Busy,
    /// 兼容自定义后端的文本错误；内置后端使用 Io 或 Database。
    Storage(String),
    /// 兼容调用方的文本错误；内置执行路径使用 Agent 或具体错误类别。
    Execution(String),
    NeedsResolution(String),
    Io(io::Error),
    #[cfg(feature = "sqlite")]
    Database(rusqlite::Error),
    Serialization(serde_json::Error),
    Trace(TraceError),
    Memory(MemoryStoreError),
    Model(ModelError),
    Agent(Box<AgentError>),
}

impl Display for RuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(f, "运行配置或状态无效：{message}"),
            Self::NotFound(id) => write!(f, "找不到运行：{id}"),
            Self::Conflict => f.write_str("运行已存在或检查点版本冲突"),
            Self::Busy => f.write_str("运行存储正在由另一个执行者使用"),
            Self::Storage(message) => write!(f, "运行存储失败：{message}"),
            Self::Execution(message) => write!(f, "运行失败：{message}"),
            Self::NeedsResolution(id) => {
                write!(f, "工具 {id} 的执行结果未知，需核实后提交结果或明确重试")
            }
            Self::Io(error) => write!(f, "运行 I/O 失败：{error}"),
            #[cfg(feature = "sqlite")]
            Self::Database(error) => write!(f, "运行数据库失败：{error}"),
            Self::Serialization(error) => write!(f, "运行 JSON 编解码失败：{error}"),
            Self::Trace(error) => write!(f, "运行追踪失败：{error}"),
            Self::Memory(error) => write!(f, "运行记忆处理失败：{error}"),
            Self::Model(error) => write!(f, "运行模型处理失败：{error}"),
            Self::Agent(error) => write!(f, "运行失败：{error}"),
        }
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            #[cfg(feature = "sqlite")]
            Self::Database(error) => Some(error),
            Self::Serialization(error) => Some(error),
            Self::Trace(error) => Some(error),
            Self::Memory(error) => Some(error),
            Self::Model(error) => Some(error),
            Self::Agent(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<io::Error> for RuntimeError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
#[cfg(feature = "sqlite")]
impl From<rusqlite::Error> for RuntimeError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}
impl From<serde_json::Error> for RuntimeError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}
impl From<TraceError> for RuntimeError {
    fn from(error: TraceError) -> Self {
        Self::Trace(error)
    }
}
impl From<MemoryStoreError> for RuntimeError {
    fn from(error: MemoryStoreError) -> Self {
        Self::Memory(error)
    }
}
impl From<ModelError> for RuntimeError {
    fn from(error: ModelError) -> Self {
        Self::Model(error)
    }
}
impl From<AgentError> for RuntimeError {
    fn from(error: AgentError) -> Self {
        match error {
            AgentError::Trace(error) => Self::Trace(error),
            AgentError::Memory(error) => Self::Memory(error),
            AgentError::Model(error) => Self::Model(error),
            error => Self::Agent(Box::new(error)),
        }
    }
}
