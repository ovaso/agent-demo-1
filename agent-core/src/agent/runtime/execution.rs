use super::super::{
    AgentError, AgentResult,
    model_step::{self, ModelStep},
};
use super::{LoopPhase, PauseReason, RunState, RunStatus, RunStore, Runtime, RuntimeError};
use crate::{
    context::{Context, Message},
    memory::MemoryStore,
    model::{ModelProvider, ModelRequest},
    tool::ToolOutput,
    trace::{RunTrace, TraceSink},
};
use serde_json::json;
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
                text,
                steps: state.budget.model_calls as usize,
                session_finished: false,
            });
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
            let response = super::planning_tools::invoke(state, &call);
            let (text, ready) = match response {
                Ok(output) => (output.text, output.plan_ready),
                Err(error) => (format!("运行时工具失败：{error}"), false),
            };
            self.accept_tool(state, ToolOutput::text(text))?;
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
        if allowed.is_none()
            || (state.intent == super::WorkIntent::PlanOnly
                && !allowed.is_some_and(|tool| tool.is_read_only()))
        {
            self.accept_tool(
                state,
                ToolOutput::text("拒绝执行：工具不在当前任务允许的能力集合中"),
            )?;
            return self.commit(state);
        }
        state.phase = LoopPhase::ToolInFlight {
            call_id: call.id().into(),
        };
        self.commit(state)?;
        trace
            .start(
                &mut self.trace,
                "tool.call",
                json!({"call_id": call.id(), "tool_name": call.name(), "logical_run_id": state.id}),
            )
            .map_err(RuntimeError::storage)?;
        let (output, error) = match self.tools.invoke(call.name(), call.arguments()) {
            Ok(output) => (output, None),
            Err(error) => (
                ToolOutput::text(format!("工具调用失败：{error}")),
                Some(AgentError::Tool(error)),
            ),
        };
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
        if output.content().len() > state.limits.max_tool_output_bytes {
            return Err(RuntimeError::Invalid(
                "工具结果字节数超限；副作用可能已发生，需核实结果".into(),
            ));
        }
        let call = state
            .pending
            .pop_front()
            .ok_or_else(|| RuntimeError::Invalid("没有待执行工具".into()))?;
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
