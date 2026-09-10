use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Display, Formatter},
};

use super::{Arguments, Parameter, Tool, ToolDefinition, ToolError, ToolOutput};

/// 存放具名工具，并将通过校验的调用路由给对应工具。
#[derive(Default)]
pub struct Registry {
    pub(super) tools: BTreeMap<String, Box<dyn Tool>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册工具；工具名和参数名都必须唯一。
    pub fn register<T>(&mut self, tool: T) -> Result<(), RegistryError>
    where
        T: Tool + 'static,
    {
        self.register_boxed(Box::new(tool))
    }

    pub(super) fn register_boxed(&mut self, tool: Box<dyn Tool>) -> Result<(), RegistryError> {
        let name = tool.name().to_owned();
        validate_definition(tool.as_ref())?;

        if self.tools.contains_key(&name) {
            return Err(RegistryError::DuplicateTool { name });
        }

        self.tools.insert(name, tool);
        Ok(())
    }

    /// 按首次加入时间、名称返回定义；版本不参与排序。
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        let mut definitions: Vec<_> = self.tools.values().map(|tool| tool.definition()).collect();
        definitions.sort_unstable_by(|left, right| left.sort_key().cmp(&right.sort_key()));
        definitions
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// 仅校验调用形状，不执行工具，用于计划预检。
    pub fn validate(&self, name: &str, arguments: &Arguments) -> Result<(), RegistryError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| RegistryError::ToolNotFound {
                name: name.to_owned(),
            })?;
        validate_arguments(name, tool.parameters(), arguments)
    }

    /// 校验参数并调用指定工具。
    pub fn invoke(&self, name: &str, arguments: &Arguments) -> Result<ToolOutput, RegistryError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| RegistryError::ToolNotFound {
                name: name.to_owned(),
            })?;

        validate_arguments(name, tool.parameters(), arguments)?;

        tool.invoke(arguments)
            .map_err(|source| RegistryError::Execution {
                tool: name.to_owned(),
                source,
            })
    }
}

#[derive(Debug)]
pub enum RegistryError {
    EmptyToolName,
    DuplicateTool {
        name: String,
    },
    EmptyParameterName {
        tool: String,
    },
    DuplicateParameter {
        tool: String,
        parameter: String,
    },
    ToolNotFound {
        name: String,
    },
    MissingArgument {
        tool: String,
        parameter: String,
    },
    UnexpectedArgument {
        tool: String,
        parameter: String,
    },
    Execution {
        tool: String,
        source: ToolError,
    },
    MissingContext {
        tool: String,
        expected: &'static str,
    },
}

impl Display for RegistryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyToolName => formatter.write_str("a tool name cannot be empty"),
            Self::DuplicateTool { name } => {
                write!(formatter, "tool `{name}` is already registered")
            }
            Self::EmptyParameterName { tool } => {
                write!(
                    formatter,
                    "tool `{tool}` has a parameter with an empty name"
                )
            }
            Self::DuplicateParameter { tool, parameter } => {
                write!(
                    formatter,
                    "tool `{tool}` has duplicate parameter `{parameter}`"
                )
            }
            Self::ToolNotFound { name } => write!(formatter, "tool `{name}` is not registered"),
            Self::MissingArgument { tool, parameter } => {
                write!(formatter, "tool `{tool}` requires argument `{parameter}`")
            }
            Self::UnexpectedArgument { tool, parameter } => {
                write!(
                    formatter,
                    "tool `{tool}` does not accept argument `{parameter}`"
                )
            }
            Self::Execution { tool, source } => write!(formatter, "tool `{tool}` failed: {source}"),
            Self::MissingContext { tool, expected } => {
                write!(formatter, "tool `{tool}` requires context `{expected}`")
            }
        }
    }
}

impl Error for RegistryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Execution { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn validate_definition(tool: &dyn Tool) -> Result<(), RegistryError> {
    let name = tool.name();
    if name.trim().is_empty() {
        return Err(RegistryError::EmptyToolName);
    }

    let mut parameter_names = BTreeSet::new();
    for parameter in tool.parameters() {
        if parameter.name().trim().is_empty() {
            return Err(RegistryError::EmptyParameterName {
                tool: name.to_owned(),
            });
        }

        if !parameter_names.insert(parameter.name()) {
            return Err(RegistryError::DuplicateParameter {
                tool: name.to_owned(),
                parameter: parameter.name().to_owned(),
            });
        }
    }

    Ok(())
}

fn validate_arguments(
    tool_name: &str,
    parameters: &[Parameter],
    arguments: &Arguments,
) -> Result<(), RegistryError> {
    for parameter in parameters {
        if parameter.is_required() && arguments.get(parameter.name()).is_none() {
            return Err(RegistryError::MissingArgument {
                tool: tool_name.to_owned(),
                parameter: parameter.name().to_owned(),
            });
        }
    }

    for (argument, _) in arguments.iter() {
        if !parameters
            .iter()
            .any(|parameter| parameter.name() == argument)
        {
            return Err(RegistryError::UnexpectedArgument {
                tool: tool_name.to_owned(),
                parameter: argument.to_owned(),
            });
        }
    }

    Ok(())
}
