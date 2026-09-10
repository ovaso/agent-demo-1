//! Agent 的配置、结果与执行循环。

pub mod blackboard;
pub mod graph;
mod model_step;
pub mod planning;
pub mod routing;
mod runner;
pub mod runtime;
#[cfg(test)]
mod tests;
mod tool_calls;

use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

use super::{
    context::{ContextStore, ContextStoreError},
    memory::{MemoryStore, MemoryStoreError},
    model::{ModelError, ModelProvider},
    tool::{Registry, RegistryError},
    trace::{NoopTraceSink, TraceError, TraceSink},
};

/// Agent loop 的执行边界。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentConfig {
    max_steps: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self { max_steps: 8 }
    }
}

impl AgentConfig {
    pub fn new(max_steps: usize) -> Self {
        Self { max_steps }
    }

    pub fn max_steps(&self) -> usize {
        self.max_steps
    }
}

/// 一次 Agent 执行的最终结果。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentResult {
    text: String,
    steps: usize,
    session_finished: bool,
}

impl AgentResult {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn steps(&self) -> usize {
        self.steps
    }

    pub fn session_finished(&self) -> bool {
        self.session_finished
    }
}

/// 编排模型、上下文、长期记忆与工具的核心对象。
pub struct Agent<M, C, S, T = NoopTraceSink> {
    pub(crate) model: M,
    pub(crate) context_store: C,
    pub(crate) memory_store: S,
    pub(crate) tools: Registry,
    pub(crate) config: AgentConfig,
    pub(crate) trace_sink: T,
}

impl<M, C, S> Agent<M, C, S, NoopTraceSink> {
    pub fn new(model: M, context_store: C, memory_store: S, tools: Registry) -> Self {
        Self {
            model,
            context_store,
            memory_store,
            tools,
            config: AgentConfig::default(),
            trace_sink: NoopTraceSink,
        }
    }

    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_trace_sink<T>(self, trace_sink: T) -> Agent<M, C, S, T> {
        Agent {
            model: self.model,
            context_store: self.context_store,
            memory_store: self.memory_store,
            tools: self.tools,
            config: self.config,
            trace_sink,
        }
    }
}

impl<M, C, S, T> Agent<M, C, S, T> {
    pub fn tools_mut(&mut self) -> &mut Registry {
        &mut self.tools
    }

    pub fn context_store(&self) -> &C {
        &self.context_store
    }

    pub fn context_store_mut(&mut self) -> &mut C {
        &mut self.context_store
    }

    pub fn memory_store(&self) -> &S {
        &self.memory_store
    }

    pub fn memory_store_mut(&mut self) -> &mut S {
        &mut self.memory_store
    }
}

#[derive(Debug)]
pub enum AgentError {
    InvalidConfiguration(String),
    Context(ContextStoreError),
    Memory(MemoryStoreError),
    Model(ModelError),
    Tool(RegistryError),
    Trace(TraceError),
    EmptyModelResponse,
    MaxStepsExceeded { max_steps: usize },
}

impl Display for AgentError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => write!(formatter, "Agent 配置无效：{message}"),
            Self::Context(error) => write!(formatter, "上下文处理失败：{error}"),
            Self::Memory(error) => write!(formatter, "长期记忆处理失败：{error}"),
            Self::Model(error) => write!(formatter, "模型调用失败：{error}"),
            Self::Tool(error) => write!(formatter, "工具调用失败：{error}"),
            Self::Trace(error) => write!(formatter, "追踪记录失败：{error}"),
            Self::EmptyModelResponse => formatter.write_str("模型既未返回文本也未请求工具调用"),
            Self::MaxStepsExceeded { max_steps } => {
                write!(formatter, "Agent 在 {max_steps} 步内未得到最终回复")
            }
        }
    }
}

impl Error for AgentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Context(error) => Some(error),
            Self::Memory(error) => Some(error),
            Self::Model(error) => Some(error),
            Self::Tool(error) => Some(error),
            Self::Trace(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ContextStoreError> for AgentError {
    fn from(error: ContextStoreError) -> Self {
        Self::Context(error)
    }
}

impl From<MemoryStoreError> for AgentError {
    fn from(error: MemoryStoreError) -> Self {
        Self::Memory(error)
    }
}

impl From<ModelError> for AgentError {
    fn from(error: ModelError) -> Self {
        Self::Model(error)
    }
}

impl From<RegistryError> for AgentError {
    fn from(error: RegistryError) -> Self {
        Self::Tool(error)
    }
}

impl From<TraceError> for AgentError {
    fn from(error: TraceError) -> Self {
        Self::Trace(error)
    }
}

impl<M, C, S, T> Agent<M, C, S, T>
where
    M: ModelProvider,
    C: ContextStore,
    S: MemoryStore,
    T: TraceSink,
{
    pub fn run(
        &mut self,
        session_id: &str,
        input: impl Into<String>,
    ) -> Result<AgentResult, AgentError> {
        runner::run(self, session_id, input.into())
    }

    pub fn run_stream(
        &mut self,
        session_id: &str,
        input: impl Into<String>,
        on_text_delta: &mut dyn FnMut(&str),
    ) -> Result<AgentResult, AgentError> {
        runner::run_stream(self, session_id, input.into(), on_text_delta)
    }
}
