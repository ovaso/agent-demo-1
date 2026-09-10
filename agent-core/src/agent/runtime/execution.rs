use super::super::AgentError;
use super::{LoopPhase, PauseReason, RunState, RunStatus, RunStore, Runtime, RuntimeError};
use crate::{
    memory::MemoryStore,
    model::ModelProvider,
    tool::ToolOutput,
    trace::{RunTrace, TraceSink},
};
use serde_json::json;

impl<M: ModelProvider, R: RunStore, S: MemoryStore, T: TraceSink> Runtime<M, R, S, T> {
    pub(super) fn call_tool(
        &mut self,
        state: &mut RunState,
        trace: &mut RunTrace,
    ) -> Result<(), RuntimeError> {
        let Some(call) = state.pending.front().cloned() else {
            state.phase = LoopPhase::Model;
            return self.commit(state);
        };
        if state.budget.tool_calls >= state.limits.max_tool_calls {
            state.status = RunStatus::Paused(PauseReason::Budget);
            return self.commit(state);
        }
        state.budget.tool_calls += 1;
        if state.planning && call.name().starts_with("runtime_") {
            trace
                .start(
                    &mut self.trace,
                    "runtime.control",
                    json!({"call_id":call.id(),"tool_name":call.name(),"logical_run_id":state.id}),
                )
                .map_err(RuntimeError::storage)?;
            let response = super::planning_tools::invoke(state, &call, &self.tools);
            if let Ok(output) = &response
                && matches!(
                    call.name(),
                    "runtime_plan" | "runtime_delegate" | "runtime_reply"
                )
            {
                super::step_budget::record_tool(state, &call, &output.text);
            }
            if let Ok(output) = &response
                && let Some(request_id) = &output.wait_request
            {
                state.phase = LoopPhase::Waiting {
                    request_id: request_id.clone(),
                };
                self.commit(state)?;
                return trace
                    .end(&mut self.trace, None)
                    .map_err(RuntimeError::storage);
            }
            let (text, ready, abort) = match response {
                Ok(output) => (output.text, output.plan_ready, output.abort_batch),
                Err(error) => (
                    format!("运行时工具失败：{error}"),
                    false,
                    matches!(call.name(), "runtime_ask" | "runtime_wait"),
                ),
            };
            self.accept_tool(state, ToolOutput::text(text))?;
            state.last_tool_succeeded = None;
            if abort {
                Self::skip_batch(state, "未执行：协作请求失败，需要重新决定");
            }
            if ready {
                for pending in state.pending.drain(..) {
                    state
                        .context
                        .push_tool(pending.id(), pending.name(), "未执行：计划已交付");
                }
                state.phase = LoopPhase::Model;
                state.status = RunStatus::Paused(PauseReason::PlanReady);
            }
            self.commit(state)?;
            return trace
                .end(&mut self.trace, None)
                .map_err(RuntimeError::storage);
        }
        let allowed = state.tools.iter().find(|tool| tool.name() == call.name());
        let read_only = allowed.is_some_and(|tool| tool.is_read_only());
        if !state.tool_allowed(call.name()) {
            self.accept_tool(
                state,
                ToolOutput::text("拒绝执行：工具不在当前任务允许的能力集合中").with_success(false),
            )?;
            state.last_tool_succeeded = Some(false);
            return self.commit(state);
        }
        state.phase = LoopPhase::ToolInFlight {
            call_id: call.id().into(),
        };
        if !read_only {
            state.work_revision += 1;
        }
        self.commit(state)?;
        trace
            .start(
                &mut self.trace,
                "tool.call",
                json!({"call_id": call.id(), "tool_name": call.name(), "logical_run_id": state.id}),
            )
            .map_err(RuntimeError::storage)?;
        let (mut output, mut error) = match self.tools.invoke(call.name(), call.arguments()) {
            Ok(output) => (output, None),
            Err(error) => (
                ToolOutput::text(format!("工具调用失败：{error}")),
                Some(AgentError::Tool(error)),
            ),
        };
        if output.finishes_session() && state.graph.unfinished() {
            output = ToolOutput::text("拒绝结束会话：图节点尚未结算，需完成任务或显式取消");
            error = Some(AgentError::InvalidConfiguration("图任务未完成".into()));
        }
        if read_only && output.content().len() > state.limits.max_tool_output_bytes {
            output = ToolOutput::text("读取结果超过字节上限，请缩小读取或搜索范围后重试。");
            error = Some(AgentError::InvalidConfiguration("只读结果过大".into()));
        }
        state.last_tool_succeeded = Some(error.is_none() && output.succeeded());
        state.last_tool_operator = false;
        if let Err(error) = self.accept_tool(state, output) {
            state.status = RunStatus::Paused(PauseReason::Limit(error.to_string()));
            self.commit(state)?;
            return Err(error);
        }
        self.commit(state)?;
        trace
            .end(&mut self.trace, error.as_ref())
            .map_err(RuntimeError::storage)
    }

    pub(super) fn accept_tool(
        &mut self,
        state: &mut RunState,
        output: ToolOutput,
    ) -> Result<(), RuntimeError> {
        if output.finishes_session() && state.graph.unfinished() {
            return Err(RuntimeError::Invalid("图节点未结算，不能结束根会话".into()));
        }
        if output.content().len() > state.limits.max_tool_output_bytes {
            return Err(RuntimeError::Invalid(
                "工具结果字节数超限；副作用可能已发生，需核实结果".into(),
            ));
        }
        let call = state
            .pending
            .pop_front()
            .ok_or_else(|| RuntimeError::Invalid("没有待执行工具".into()))?;
        if !call.name().starts_with("runtime_")
            && state.last_tool_succeeded == Some(true)
            && output.succeeded()
        {
            super::step_budget::record_tool(state, &call, output.content());
        }
        state
            .context
            .push_tool(call.id(), call.name(), output.content());
        if output.finishes_session() {
            for remaining in state.pending.drain(..) {
                state.context.push_tool(
                    remaining.id(),
                    remaining.name(),
                    "未执行：会话结束请求已生效",
                );
            }
            state.phase = LoopPhase::FinishSession {
                summary: output.content().into(),
            };
        } else {
            state.phase = if state.pending.is_empty() {
                LoopPhase::Model
            } else {
                LoopPhase::Tools
            };
        }
        Ok(())
    }

    pub(super) fn skip_batch(state: &mut RunState, reason: &str) {
        for call in state.pending.drain(..) {
            state.context.push_tool(call.id(), call.name(), reason);
        }
        state.phase = LoopPhase::Model;
    }
}
