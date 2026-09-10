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
        if !state.goal.is_empty() && plan.goal != state.goal {
            return Err(RuntimeError::Invalid("计划目标必须与根任务一致".into()));
        }
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

    /// 将已交付的计划转为执行；保留原上下文、工作板和已消耗预算。
    pub fn execute_plan(&mut self, id: &str) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        if state.intent != super::WorkIntent::PlanOnly
            || state.status != super::RunStatus::Paused(super::PauseReason::PlanReady)
            || !state.pending.is_empty()
            || state.plans.current().is_none()
        {
            return Err(RuntimeError::Invalid(
                "需要一份已交付且工具批次结算完毕的计划".into(),
            ));
        }
        state.intent = super::WorkIntent::Execute;
        state.status = super::RunStatus::Running;
        state.context.push_user(format!(
            "执行已保存的计划 v{}，继续原任务并遵守既有验收条件。",
            state.plans.revision()
        ));
        self.commit(&mut state)?;
        Ok(state)
    }
}
