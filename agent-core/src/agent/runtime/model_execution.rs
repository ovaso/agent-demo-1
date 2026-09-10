use super::super::{
    AgentError, AgentResult,
    model_step::{self, ModelStep},
};
use super::{LoopPhase, PauseReason, RunState, RunStatus, RunStore, Runtime, RuntimeError};
use crate::{
    context::{Context, Message},
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
            state.status = RunStatus::Paused(PauseReason::Budget);
            return self.commit(state);
        }
        if let Err(error) = validate_protocol(&state.context).and_then(|()| {
            super::store::bounded_json(&state.context, state.limits.max_context_bytes)
        }) {
            state.status = RunStatus::Paused(PauseReason::Limit(error.to_string()));
            self.commit(state)?;
            return Err(error);
        }
        let (messages, tools) = match super::planning_prompt::request_context(state) {
            Ok(request) => request,
            Err(error) => {
                state.status = RunStatus::Paused(PauseReason::Limit(error.to_string()));
                self.commit(state)?;
                return Err(error);
            }
        };
        state.budget.model_calls += 1;
        state.phase = LoopPhase::ModelInFlight;
        self.commit(state)?;
        let request = ModelRequest::new(messages, &state.memories, &tools);
        let response = model_step::stream(
            &mut self.model,
            &mut self.trace,
            trace,
            ModelStep {
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
        let (text, calls) = response.into_parts();
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
        if calls.is_empty() {
            let Some(text) = text else {
                state.status = RunStatus::Paused(PauseReason::Model("模型响应为空".into()));
                self.commit(state)?;
                return Err(AgentError::EmptyModelResponse.into());
            };
            state.context.push_assistant(&text);
            if state.intent == super::WorkIntent::PlanOnly {
                state.status = RunStatus::Paused(if state.plans.current().is_some() {
                    PauseReason::PlanReady
                } else {
                    PauseReason::Model("需要通过 runtime_plan 提交结构化计划".into())
                });
                return self.commit(state);
            }
            state.result = Some(AgentResult {
                // The coordinator is the only actor that completes the root run.
                text,
                steps: state.budget.model_calls as usize,
                session_finished: false,
            });
            if state.graph.active.is_some() {
                let text = state.result.take().expect("model result").text;
                super::graph_execution::finish_node(
                    state,
                    super::super::graph::NodeStatus::Succeeded,
                    Some(text),
                    Some(super::super::graph::ValidationKind::ModelReported),
                )?;
                return self.commit(state);
            }
            if state.graph.unfinished() {
                state.result = None;
                state.status = RunStatus::Paused(PauseReason::GraphBlocked(
                    "仍有未通过的图节点；请继续、重试或重新规划".into(),
                ));
                return self.commit(state);
            }
            state.phase = LoopPhase::Done;
            state.status = RunStatus::Completed;
        } else {
            state
                .context
                .push_assistant_with_tool_calls(text.unwrap_or_default(), calls.clone());
            state.pending = calls.into();
            state.phase = LoopPhase::Tools;
        }
        self.commit(state)
    }
}

/// 模型请求前验证调用与结果成组闭合，防止旧数据或手工上下文破坏协议。
fn validate_protocol(context: &Context) -> Result<(), RuntimeError> {
    let mut pending = std::collections::BTreeMap::new();
    for message in context.messages() {
        match message {
            Message::Tool { call_id, name, .. } => {
                if pending.remove(call_id.as_str()) != Some(name.as_str()) {
                    return Err(RuntimeError::Invalid("工具结果没有匹配的调用".into()));
                }
            }
            _ => {
                if !pending.is_empty() {
                    return Err(RuntimeError::Invalid("工具批次未完成".into()));
                }
                if let Some(calls) = message.tool_calls() {
                    for call in calls {
                        if pending.insert(call.id(), call.name()).is_some() {
                            return Err(RuntimeError::Invalid("工具调用 ID 重复".into()));
                        }
                    }
                }
            }
        }
    }
    if pending.is_empty() {
        Ok(())
    } else {
        Err(RuntimeError::Invalid("工具批次未完成".into()))
    }
}
