use super::super::{blackboard::BoardUpdate, planning::Plan};
use super::{RunState, RunStore, Runtime, RuntimeError};
use crate::{memory::MemoryStore, model::ModelProvider, trace::TraceSink};

impl<M: ModelProvider, R: RunStore, S: MemoryStore, T: TraceSink> Runtime<M, R, S, T> {
    pub fn propose_plan(
        &mut self,
        id: &str,
        expected_revision: u64,
        plan: Plan,
    ) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        state.plans.propose(expected_revision, plan)?;
        self.commit(&mut state)?;
        Ok(state)
    }

    pub fn write_board(
        &mut self,
        id: &str,
        author: &str,
        update: BoardUpdate,
    ) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        state
            .blackboard
            .write(author, state.plans.revision(), update)?;
        self.commit(&mut state)?;
        Ok(state)
    }
}
