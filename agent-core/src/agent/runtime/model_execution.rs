use super::super::{
    AgentError, AgentResult,
    model_step::{self, ModelStep},
};
use super::{LoopPhase, PauseReason, RunState, RunStatus, RunStore, Runtime, RuntimeError};
use crate::{
    context::Message,
    memory::MemoryStore,
    model::{ModelProvider, ModelRequest},
    trace::{RunTrace, TraceSink},
};
use std::collections::BTreeSet;

impl<M: ModelProvider, R: RunStore, S: MemoryStore, T: TraceSink> Runtime<M, R, S, T> {
    pub(super) fn call_model(
        &mut self,
        state: &mut RunState,
        on_text: &mut dyn FnMut(&str),
        trace: &mut RunTrace,
    ) -> Result<(), RuntimeError> {
        if state.budget.model_calls >= state.limits.max_steps {
            if let Some(grant) = super::step_budget::extend(state) {
                // Persist the grant before another model request; a restart cannot
                // spend the same progress twice or reset the extension counter.
                self.commit(state)?;
                trace
                    .record(
                        &mut self.trace,
                        crate::trace::TraceEvent::new("runtime.budget.extended")
                            .with_field("logical_run_id", state.id())
                            .with_field("grant", serde_json::json!(grant)),
                    )
                    .map_err(RuntimeError::storage)?;
            } else {
                state.status = RunStatus::Paused(PauseReason::Budget);
                return self.commit(state);
            }
        }
        if state
            .agent_policy()
            .is_some_and(|policy| policy.model_calls >= policy.max_steps)
        {
            super::graph_execution::finish_node(
                state,
                super::super::graph::NodeStatus::BudgetExceeded,
                Some("局部模型步数已用完，需要协调者调整额度或取消。".into()),
                None,
            )?;
            return self.commit(state);
        }
        let (messages, tools, change, compacted) = match super::model_input::prepare(state) {
            Ok(prepared) => prepared,
            Err(error) => {
                state.status = RunStatus::Paused(PauseReason::Limit(error.to_string()));
                self.commit(state)?;
                return Err(error);
            }
        };
        if let Some(compacted) = compacted {
            trace
                .record(
                    &mut self.trace,
                    crate::trace::TraceEvent::new("runtime.context.compacted")
                        .with_field("actor", state.actor())
                        .with_field("details", serde_json::json!(compacted)),
                )
                .map_err(RuntimeError::storage)?;
        }
        if let Some(change) = change {
            trace
                .record(
                    &mut self.trace,
                    crate::trace::TraceEvent::new("runtime.prompt.updated")
                        .with_field("details", serde_json::json!(change)),
                )
                .map_err(RuntimeError::storage)?;
        }
        state.budget.model_calls += 1;
        if let Some(id) = state.graph.active.clone()
            && let Some(policy) = state
                .graph
                .current_mut()
                .and_then(|run| run.nodes.get_mut(&id))
                .and_then(|node| node.policy.as_mut())
        {
            policy.model_calls += 1;
        }
        state.phase = LoopPhase::ModelInFlight;
        self.commit(state)?;
        let request = ModelRequest::new(messages, &[], &tools)
            .with_max_input_bytes(state.limits.max_context_bytes);
        let actor = state.actor();
        let response = model_step::stream(
            &mut self.model,
            &mut self.trace,
            trace,
            ModelStep {
                actor: &actor,
                request,
                session_id: &state.session_id,
                step: state.budget.model_calls as usize,
            },
            Some(on_text),
        );
        state.phase = LoopPhase::Model;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                state.status = RunStatus::Paused(PauseReason::Model(error.to_string()));
                self.commit(state)?;
                return Err(error.into());
            }
        };
        let stop = response.stop_reason().clone();
        let (text, calls, continuation) = response.into_reply_parts();
        let mut ids = BTreeSet::new();
        if calls.len() > state.limits.max_calls_per_response
            || calls
                .iter()
                .any(|call| call.id().is_empty() || !ids.insert(call.id()))
            || text
                .as_ref()
                .is_some_and(|text| text.len() > state.limits.max_context_bytes)
        {
            state.status =
                RunStatus::Paused(PauseReason::Limit("模型响应过大或工具调用 ID 无效".into()));
            self.commit(state)?;
            return Err(RuntimeError::Invalid("模型响应被拒绝，未执行工具".into()));
        }
        if !stop.is_complete() {
            if let Some(text) = text {
                state.context.push_assistant(text);
            }
            state.context.push_user(format!("上一轮响应未完成（{}）。恢复后只继续未完成工作，不要把截断的输出或工具调用当作已执行。", stop.description()));
            state.status = RunStatus::Paused(PauseReason::Model(stop.description().into()));
            return self.commit(state);
        }
        if calls.is_empty() {
            let Some(text) = text else {
                state.status = RunStatus::Paused(PauseReason::Model("模型响应为空".into()));
                self.commit(state)?;
                return Err(AgentError::EmptyModelResponse.into());
            };
            state
                .context
                .push(Message::assistant_reply(&text, Vec::new(), continuation));
            if state.graph.active.is_some() {
                super::graph_execution::finish_node(
                    state,
                    super::super::graph::NodeStatus::Succeeded,
                    Some(text),
                    Some(super::super::graph::ValidationKind::ModelReported),
                )?;
                return self.commit(state);
            }
            if state.graph.unfinished() {
                state.status =
                    RunStatus::Paused(PauseReason::GraphBlocked("仍有未完成工作或委托".into()));
                return self.commit(state);
            }
            if state.intent == super::WorkIntent::PlanOnly {
                state.status = RunStatus::Paused(if state.plans.current().is_some() {
                    PauseReason::PlanReady
                } else {
                    PauseReason::Model("需要提交结构化计划".into())
                });
                return self.commit(state);
            }
            state.result = Some(AgentResult {
                text,
                steps: state.budget.model_calls as usize,
                session_finished: false,
            });
            state.phase = LoopPhase::Done;
            state.status = RunStatus::Completed;
        } else {
            state.context.push(Message::assistant_reply(
                text.unwrap_or_default(),
                calls.clone(),
                continuation,
            ));
            state.pending = calls.into();
            state.phase = LoopPhase::Tools;
        }
        self.commit(state)
    }
}
