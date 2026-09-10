//! 可暂停、恢复并逐工具提交检查点的同步运行时。

mod agent_tools;
mod budget;
mod collaboration;
#[cfg(test)]
mod collaboration_tests;
mod control;
mod delegation;
#[cfg(test)]
mod delegation_tests;
mod engine;
mod error;
mod execution;
mod graph_control;
mod graph_execution;
#[cfg(test)]
mod graph_tests;
mod memory_input;
mod message_delivery;
mod message_tools;
mod model_execution;
mod model_input;
mod options;
mod planning_prompt;
#[cfg(test)]
mod planning_tests;
mod planning_tools;
mod planning_view;
mod prompt_history;
mod serialization;
#[cfg(feature = "sqlite")]
mod sqlite;
#[cfg(all(test, feature = "sqlite"))]
mod sqlite_tests;
mod state;
mod step_budget;
#[cfg(test)]
mod step_budget_tests;
mod store;
#[cfg(test)]
mod tests;
mod workspace;

pub use budget::{RunBudget, RunLimits};
pub use error::RuntimeError;
pub use options::{RunOptions, WorkIntent};
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteRunStore;
pub use state::{LoopPhase, PauseReason, RunState, RunStatus};
pub use step_budget::{StepExtension, StepExtensionBlock, StepExtensionPolicy};
pub use store::{MemoryRunStore, RunLease, RunStore};

use crate::{tool::Registry, trace::NoopTraceSink};

pub struct Runtime<M, R, S, T = NoopTraceSink> {
    model: M,
    store: R,
    memory: S,
    tools: Registry,
    trace: T,
}

impl<M, R, S> Runtime<M, R, S> {
    pub fn new(model: M, store: R, memory: S, tools: Registry) -> Self {
        Self {
            model,
            store,
            memory,
            tools,
            trace: NoopTraceSink,
        }
    }
}

impl<M, R, S, T> Runtime<M, R, S, T> {
    pub fn with_trace_sink<U>(self, trace: U) -> Runtime<M, R, S, U> {
        Runtime {
            model: self.model,
            store: self.store,
            memory: self.memory,
            tools: self.tools,
            trace,
        }
    }
    pub fn store(&self) -> &R {
        &self.store
    }
    pub fn store_mut(&mut self) -> &mut R {
        &mut self.store
    }
}

impl<M, R: RunStore, S, T> Runtime<M, R, S, T> {
    pub fn state(&self, id: &str) -> Result<RunState, RuntimeError> {
        let state = self
            .store
            .load(id)?
            .ok_or_else(|| RuntimeError::NotFound(id.into()))?;
        state.validate()?;
        Ok(state)
    }

    fn commit(&mut self, state: &mut RunState) -> Result<(), RuntimeError> {
        let revision = state.revision;
        state.revision = revision
            .checked_add(1)
            .ok_or_else(|| RuntimeError::Invalid("检查点版本溢出".into()))?;
        self.store.save(state, revision)
    }
}
