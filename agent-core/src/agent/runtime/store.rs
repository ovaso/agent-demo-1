use super::{RunState, RuntimeError};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// 覆盖模型和工具执行的排他所有权；释放所有权不撤销已产生的外部副作用。
pub struct RunLease {
    pub(crate) memory: Option<Arc<AtomicBool>>,
    #[cfg(feature = "sqlite")]
    pub(crate) file: Option<std::fs::File>,
}

impl Drop for RunLease {
    fn drop(&mut self) {
        if let Some(busy) = &self.memory {
            busy.store(false, Ordering::Release);
        }
        #[cfg(feature = "sqlite")]
        if let Some(file) = &self.file {
            let _ = file.unlock();
        }
    }
}

/// 原子保存完整执行检查点。自定义后端必须实现排他执行和版本比较。
pub trait RunStore {
    type Lease;
    fn acquire(&self) -> Result<Self::Lease, RuntimeError>;
    fn load(&self, run_id: &str) -> Result<Option<RunState>, RuntimeError>;
    fn create(&mut self, state: &RunState) -> Result<(), RuntimeError>;
    fn save(&mut self, state: &RunState, expected_revision: u64) -> Result<(), RuntimeError>;
}

#[derive(Default)]
pub struct MemoryRunStore {
    states: BTreeMap<String, RunState>,
    busy: Arc<AtomicBool>,
}

impl MemoryRunStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl RunStore for MemoryRunStore {
    type Lease = RunLease;
    fn acquire(&self) -> Result<RunLease, RuntimeError> {
        self.busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| RuntimeError::Busy)?;
        Ok(RunLease {
            memory: Some(Arc::clone(&self.busy)),
            #[cfg(feature = "sqlite")]
            file: None,
        })
    }
    fn load(&self, run_id: &str) -> Result<Option<RunState>, RuntimeError> {
        Ok(self.states.get(run_id).cloned())
    }
    fn create(&mut self, state: &RunState) -> Result<(), RuntimeError> {
        check_size(state)?;
        if self.states.contains_key(state.id())
            || self.states.values().any(|existing| {
                existing.session_id == state.session_id
                    && !matches!(
                        existing.status,
                        super::RunStatus::Completed | super::RunStatus::Cancelled
                    )
            })
        {
            return Err(RuntimeError::Conflict);
        }
        self.states.insert(state.id().to_owned(), state.clone());
        Ok(())
    }
    fn save(&mut self, state: &RunState, expected_revision: u64) -> Result<(), RuntimeError> {
        check_size(state)?;
        if self.states.get(state.id()).map(RunState::revision) != Some(expected_revision)
            || expected_revision.checked_add(1) != Some(state.revision)
        {
            return Err(RuntimeError::Conflict);
        }
        self.states.insert(state.id().to_owned(), state.clone());
        Ok(())
    }
}

pub(crate) fn check_size(state: &RunState) -> Result<(), RuntimeError> {
    state.validate()?;
    super::serialization::check(state, state.limits.max_checkpoint_bytes)
}
