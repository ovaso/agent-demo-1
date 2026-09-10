use super::{LoopPhase, PauseReason, RunState, RunStatus, RunStore, Runtime, RuntimeError};
use crate::{
    memory::{Memory, MemoryStore},
    model::ModelProvider,
    trace::{RunTrace, TraceSink},
};
use serde_json::json;

impl<M: ModelProvider, R: RunStore, S: MemoryStore, T: TraceSink> Runtime<M, R, S, T> {
    /// 推进一个可检查的执行阶段；暂停状态需显式 resume。
    pub fn advance(
        &mut self,
        id: &str,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<RunState, RuntimeError> {
        self.drive(id, on_text, false)
    }

    /// 沿用原输入与预算，直到完成、暂停或失败；不会自动重跑未知工具。
    pub fn resume(
        &mut self,
        id: &str,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<RunState, RuntimeError> {
        self.drive(id, on_text, true)
    }

    fn drive(
        &mut self,
        id: &str,
        on_text: &mut dyn FnMut(&str),
        continuous: bool,
    ) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        let definitions = self.tools.definitions();
        if self.model.model_name() != state.model_name {
            return Err(RuntimeError::Invalid("模型或工具定义与检查点不一致".into()));
        }
        // Preserve the saved capability set. Source metadata can be backfilled
        // or refreshed without granting tools or relaxing executable contracts.
        for saved in &mut state.tools {
            let current = definitions
                .iter()
                .find(|current| current.name() == saved.name())
                .filter(|current| saved.same_contract(current))
                .ok_or_else(|| RuntimeError::Invalid("模型或工具定义与检查点不一致".into()))?;
            saved.refresh_metadata(current);
        }
        state
            .tools
            .sort_unstable_by(|left, right| left.sort_key().cmp(&right.sort_key()));
        if state.status == RunStatus::Paused(PauseReason::PlanReady) {
            return Ok(state);
        }
        if let LoopPhase::ToolInFlight { call_id } = &state.phase {
            let id = call_id.clone();
            state.status = RunStatus::Paused(PauseReason::ToolResultUnknown(id.clone()));
            self.commit(&mut state)?;
            return Err(RuntimeError::NeedsResolution(id));
        }
        if !continuous && !matches!(state.status, RunStatus::Running) {
            return Ok(state);
        }
        if continuous {
            state.status = RunStatus::Running;
        }
        // A request interrupted before its response was committed consumed its attempt.
        if state.phase == LoopPhase::ModelInFlight {
            state.budget.token_usage.settle(Default::default());
            state.phase = LoopPhase::Model;
            self.commit(&mut state)?;
        }
        let mut trace = RunTrace::new(&state.session_id);
        trace
            .start(
                &mut self.trace,
                "agent.run",
                json!({"logical_run_id": state.id, "checkpoint_revision": state.revision}),
            )
            .map_err(RuntimeError::storage)?;
        let result = (|| {
            loop {
                self.step(&mut state, on_text, &mut trace)?;
                if !continuous || state.status != RunStatus::Running {
                    return Ok(());
                }
            }
        })();
        let trace_error = result.as_ref().err().map(|error: &RuntimeError| {
            super::super::AgentError::InvalidConfiguration(error.to_string())
        });
        let recorded = trace
            .finish(&mut self.trace, trace_error.as_ref())
            .map_err(RuntimeError::storage);
        result?;
        recorded?;
        Ok(state)
    }

    fn step(
        &mut self,
        state: &mut RunState,
        on_text: &mut dyn FnMut(&str),
        trace: &mut RunTrace,
    ) -> Result<(), RuntimeError> {
        if state.budget.transitions >= state.limits.max_transitions {
            state.status = RunStatus::Paused(PauseReason::Budget);
            return self.commit(state);
        }
        state.budget.transitions += 1;
        if self.graph_step(state, trace)? {
            return Ok(());
        }
        match &state.phase {
            LoopPhase::Model => self.call_model(state, on_text, trace),
            LoopPhase::Tools => self.call_tool(state, trace),
            LoopPhase::FinishSession { summary } => {
                let summary = summary.clone();
                // Stable ID makes finalization replayable after memory.save succeeds.
                self.memory
                    .save(
                        Memory::new(format!("session-summary:{}", state.id), &summary)
                            .with_tag("session-summary"),
                    )
                    .map_err(RuntimeError::storage)?;
                state.result = Some(super::super::AgentResult {
                    text: summary,
                    steps: state.budget.model_calls as usize,
                    session_finished: true,
                });
                state.phase = LoopPhase::Done;
                state.status = RunStatus::Completed;
                self.commit(state)
            }
            _ => Err(RuntimeError::Invalid("不能推进当前执行阶段".into())),
        }
    }
}
