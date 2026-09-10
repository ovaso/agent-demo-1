use super::{RunLease, RunStore, check_size};
use crate::agent::runtime::RunStatus;
use crate::agent::runtime::{RunState, RuntimeError};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

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
                    && !matches!(existing.status, RunStatus::Completed | RunStatus::Cancelled)
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
