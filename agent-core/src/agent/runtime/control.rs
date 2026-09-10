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
        context: Context,
        limits: RunLimits,
    ) -> Result<RunState, RuntimeError> {
        self.start_with_options(
            id,
            session_id,
            input,
            context,
            super::RunOptions {
                limits,
                ..Default::default()
            },
        )
    }

    pub fn start_with_options(
        &mut self,
        id: &str,
        session_id: &str,
        input: &str,
        mut context: Context,
        options: super::RunOptions,
    ) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let limits = options.limits;
        limits.validate()?;
        if options.intent == super::WorkIntent::PlanOnly && !options.planning {
            return Err(RuntimeError::Invalid("只规划任务必须启用规划能力".into()));
        }
        if options.planning && limits.max_tool_output_bytes < 16 * 1024 {
            return Err(RuntimeError::Invalid(
                "规划工具结果上限至少为 16 KiB".into(),
            ));
        }
        if options.planning
            && self
                .tools
                .definitions()
                .iter()
                .any(|tool| tool.name().starts_with("runtime_"))
        {
            return Err(RuntimeError::Invalid(
                "runtime_ 工具名前缀保留给运行时".into(),
            ));
        }
        if id.trim().is_empty() || session_id.trim().is_empty() {
            return Err(RuntimeError::Invalid("运行和会话 ID 不能为空".into()));
        }
        if self.store.load(id)?.is_some() {
            return Err(RuntimeError::Conflict);
        }
        context.push_user(input);
        super::serialization::check(&context, limits.max_context_bytes)?;
        let state = RunState {
            prompt_history: Default::default(),
            collaboration: Default::default(),
            delegations_created: 0,
            graph: Default::default(),
            routing: Default::default(),
            requested_node: None,
            last_tool_succeeded: None,
            last_tool_operator: false,
            work_revision: 0,
            goal: input.into(),
            intent: options.intent,
            planning: options.planning,
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
        if state.graph.active.is_some() {
            super::graph_execution::finish_node(
                &mut state,
                super::super::graph::NodeStatus::Cancelled,
                None,
                None,
            )?;
        }
        if let Some(graph) = state.graph.current_mut() {
            for node in graph.nodes.values_mut() {
                if !matches!(
                    node.status,
                    super::super::graph::NodeStatus::Succeeded
                        | super::super::graph::NodeStatus::Failed
                ) {
                    node.status = super::super::graph::NodeStatus::Cancelled;
                }
            }
        }
        state.status = RunStatus::Cancelled;
        super::message_delivery::cancel_all(&mut state, "根任务已取消");
        self.commit(&mut state)?;
        Ok(state)
    }

    /// 显式设置固定的模型总额度并关闭自动续期；保留用量和历史，不隐含恢复。
    pub fn set_max_steps(&mut self, id: &str, max_steps: u64) -> Result<RunState, RuntimeError> {
        let _lease = self.store.acquire()?;
        let mut state = self.state(id)?;
        Self::check_editable(&state)?;
        if max_steps == 0 || max_steps < state.budget.model_calls {
            return Err(RuntimeError::Invalid("新额度不能小于已消耗步数".into()));
        }
        state.limits.max_steps = max_steps;
        // An explicit total is a fixed operator allocation; history is retained.
        state.limits.step_extension = None;
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
        let succeeded = output.succeeded();
        state.last_tool_succeeded = Some(succeeded);
        state.last_tool_operator = true;
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
