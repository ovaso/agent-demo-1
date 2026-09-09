use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    Invalid(String),
    NotFound(String),
    Conflict,
    Busy,
    Storage(String),
    Execution(String),
    NeedsResolution(String),
}

impl RuntimeError {
    pub(crate) fn storage(error: impl Display) -> Self {
        Self::Storage(error.to_string())
    }
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
        }
    }
}

impl Error for RuntimeError {}

impl From<super::super::AgentError> for RuntimeError {
    fn from(error: super::super::AgentError) -> Self {
        Self::Execution(error.to_string())
    }
}
