use super::{
    LoopPhase, PauseReason, RunBudget, RunLimits, RunState, RunStatus, RunStore, Runtime,
    RuntimeError,
};
use crate::{
    context::Context, memory::MemoryStore, model::ModelProvider, tool::ToolOutput, trace::TraceSink,
};
use std::collections::VecDeque;

impl<M: ModelProvider, R: RunStore, S: MemoryStore, T: TraceSink> Runtime<M, R, S, T> {
    /// 仅建立检查点；执行由 advance 或 resume 推进。
    pub fn start(
        &mut self,
        id: &str,
        session_id: &str,
        input: &str,
        mut context: Context,
        limits: RunLimits,
    ) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        limits.validate()?;
        if id.trim().is_empty() || session_id.trim().is_empty() {
            return Err(RuntimeError::Invalid("运行和会话 ID 不能为空".into()));
        }
        if self.store.load(id)?.is_some() {
            return Err(RuntimeError::Conflict);
        }
        context.push_user(input);
        super::store::bounded_json(&context, limits.max_context_bytes)?;
        let state = RunState {
            plans: Default::default(),
            blackboard: Default::default(),
            format_version: super::state::FORMAT_VERSION,
            revision: 0,
            id: id.into(),
            session_id: session_id.into(),
            model_name: self.model.model_name().into(),
            tools: self.tools.definitions(),
            context,
            memories: self.memory.search(input).map_err(RuntimeError::storage)?,
            limits,
            budget: RunBudget::default(),
            phase: LoopPhase::Model,
            status: RunStatus::Running,
            pending: VecDeque::new(),
            result: None,
        };
        state.validate()?;
        self.store.create(&state)?;
        Ok(state)
    }

    pub fn pause(&mut self, id: &str) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        state.status = RunStatus::Paused(PauseReason::User);
        self.commit(&mut state)?;
        Ok(state)
    }

    pub fn cancel(&mut self, id: &str) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        if let LoopPhase::ToolInFlight { call_id } = &state.phase {
            return Err(RuntimeError::NeedsResolution(call_id.clone()));
        }
        for call in state.pending.drain(..) {
            state
                .context
                .push_tool(call.id(), call.name(), "未执行：任务已取消");
        }
        state.status = RunStatus::Cancelled;
        self.commit(&mut state)?;
        Ok(state)
    }

    /// 显式调整模型总额度；不是新增额度，也不隐含恢复。
    pub fn set_max_steps(&mut self, id: &str, max_steps: u64) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        if max_steps == 0 || max_steps < state.budget.model_calls {
            return Err(RuntimeError::Invalid("新额度不能小于已消耗步数".into()));
        }
        state.limits.max_steps = max_steps;
        self.commit(&mut state)?;
        Ok(state)
    }

    /// 调用方已核实外部操作结果后提交；不会再次调用工具。
    pub fn resolve_tool(
        &mut self,
        id: &str,
        call_id: &str,
        output: ToolOutput,
    ) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        if state.phase
            != (LoopPhase::ToolInFlight {
                call_id: call_id.into(),
            })
        {
            return Err(RuntimeError::Invalid("不是当前待核实工具".into()));
        }
        self.accept_tool(&mut state, output)?;
        state.status = RunStatus::Paused(PauseReason::User);
        self.commit(&mut state)?;
        Ok(state)
    }

    /// 仅在调用方确认可以安全重试时使用；模型没有此控制入口。
    pub fn retry_tool(&mut self, id: &str, call_id: &str) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        if state.phase
            != (LoopPhase::ToolInFlight {
                call_id: call_id.into(),
            })
        {
            return Err(RuntimeError::Invalid("不是当前待核实工具".into()));
        }
        state.phase = LoopPhase::Tools;
        state.status = RunStatus::Paused(PauseReason::User);
        self.commit(&mut state)?;
        Ok(state)
    }

    pub(super) fn check_editable(state: &RunState) -> Result<(), RuntimeError> {
        if matches!(state.status, RunStatus::Completed | RunStatus::Cancelled) {
            return Err(RuntimeError::Invalid("已结束的任务不可恢复或修改".into()));
        }
        Ok(())
    }
}
