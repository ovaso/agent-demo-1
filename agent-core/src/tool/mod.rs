//! Agent 工具定义与运行时注册表。

mod registry;

use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Display, Formatter},
};

use serde::{Deserialize, Serialize};

pub use registry::{Registry, RegistryError};

/// 描述工具接受的一个输入参数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parameter {
    name: String,
    description: String,
    required: bool,
}

impl Parameter {
    pub fn required(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            required: true,
        }
    }

    pub fn optional(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            required: false,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn is_required(&self) -> bool {
        self.required
    }
}

/// LLM 选择和调用工具所需的元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDefinition {
    name: String,
    description: String,
    parameters: Vec<Parameter>,
}

impl ToolDefinition {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn parameters(&self) -> &[Parameter] {
        &self.parameters
    }
}

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

/// 一次成功的工具调用结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    content: String,
    finish_session: bool,
}

impl ToolOutput {
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            finish_session: false,
        }
    }

    pub fn finish_session(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            finish_session: true,
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn finishes_session(&self) -> bool {
        self.finish_session
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

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_owned(),
            description: self.description().to_owned(),
            parameters: self.parameters().to_vec(),
        }
    }
}
