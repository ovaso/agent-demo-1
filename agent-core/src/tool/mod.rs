//! Agent 工具定义与运行时注册表。

mod automatic;
mod definition;
pub use definition::{Parameter, ToolDefinition};
#[doc(hidden)]
pub mod invocation;
mod registry;

#[doc(hidden)]
pub use automatic::ToolRegistration;

use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Display, Formatter},
};

use serde::{Deserialize, Serialize};

pub use registry::{Registry, RegistryError};

/// 调用工具时传入的参数值。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arguments {
    values: BTreeMap<String, String>,
}

impl Arguments {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.values.insert(name.into(), value.into());
        self
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

/// 模型请求执行的一次具名工具调用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    id: String,
    name: String,
    arguments: Arguments,
}

impl ToolCall {
    pub fn new(id: impl Into<String>, name: impl Into<String>, arguments: Arguments) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn arguments(&self) -> &Arguments {
        &self.arguments
    }
}

/// 工具已返回的结果；成功返回传输结果不代表业务检查通过。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    content: String,
    finish_session: bool,
    succeeded: bool,
}

impl ToolOutput {
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            finish_session: false,
            succeeded: true,
        }
    }

    pub fn finish_session(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            finish_session: true,
            succeeded: true,
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn finishes_session(&self) -> bool {
        self.finish_session && self.succeeded
    }
    pub fn with_success(mut self, succeeded: bool) -> Self {
        self.succeeded = succeeded;
        self
    }
    pub fn succeeded(&self) -> bool {
        self.succeeded
    }
}

/// 具体工具运行时产生的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    message: String,
}

impl ToolError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for ToolError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ToolError {}

/// 可被 Agent 发现并调用的工具。
///
/// 该 trait 有意定义行为：每个实现可以执行完全不同的工作，而注册表
/// 负责发现、参数校验等共享职责。
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> &[Parameter];
    fn invoke(&self, arguments: &Arguments) -> Result<ToolOutput, ToolError>;
    /// 仅纯读取能力可显式声明；默认在 Plan Mode 中禁止执行。
    fn is_read_only(&self) -> bool {
        false
    }

    /// Fixed first-introduction Unix timestamp. Zero is reserved for legacy tools.
    fn created_at(&self) -> u64 {
        0
    }
    /// Reference version only; does not affect ordering or execution permissions.
    fn version(&self) -> &str {
        ""
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(self.name(), self.description(), self.parameters().to_vec())
            .with_metadata(self.created_at(), self.version())
            .with_read_only(self.is_read_only())
    }
}
