//! 可暂停、恢复并逐工具提交检查点的同步运行时。

mod budget;
mod control;
mod coordination;
mod error;
mod execution;
mod options;
mod prompt;
mod serialization;
mod state;
mod store;
mod tools;
mod workspace;

pub use budget::steps::{StepExtension, StepExtensionBlock, StepExtensionPolicy};
pub use budget::tokens::TokenUsage;
pub use budget::{RunBudget, RunLimits};
pub use error::RuntimeError;
pub use options::{RunOptions, WorkIntent};
pub use state::{LoopPhase, PauseReason, RunState, RunStatus};
#[cfg(feature = "sqlite")]
pub use store::sqlite::SqliteRunStore;
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
