use super::super::{
    graph::{NodeStatus, ValidationKind},
    planning::{TaskAction, ToolCheck},
    routing::ExecutionMode,
};
use super::{LoopPhase, RunState, RunStatus, RunStore, Runtime, RuntimeError};
use crate::{
    context::{Context, Message},
    memory::MemoryStore,
    model::ModelProvider,
    tool::{Arguments, ToolCall},
    trace::{RunTrace, TraceSink},
};
use std::mem;

impl<M: ModelProvider, R: RunStore, S: MemoryStore, T: TraceSink> Runtime<M, R, S, T> {
    /// true 表示本次调度已推进图阶段，false 表示交回协调者 Loop。
    pub(super) fn graph_step(
        &mut self,
        state: &mut RunState,
        trace: &mut RunTrace,
    ) -> Result<bool, RuntimeError> {
        super::message_delivery::tick(state, super::collaboration::now_ms());
        super::graph_control::apply_route(state)?;
        if let Some(id) = state.graph.active.clone() {
            if let LoopPhase::Waiting { request_id } = state.phase.clone() {
                match super::message_delivery::wait_result(state, &request_id)? {
                    Some(result) => {
                        self.accept_tool(state, crate::tool::ToolOutput::text(result.text))?;
                        state.last_tool_succeeded = None;
                        if !result.succeeded {
                            Self::skip_batch(state, "未执行：协作请求失败，需要重新决定");
                        }
                    }
                    None => finish_node(state, NodeStatus::Waiting, None, None)?,
                }
                self.commit(state)?;
                return Ok(true);
            }
            let action = state
                .graph
                .current()
                .and_then(|graph| graph.nodes.get(&id))
                .ok_or_else(|| RuntimeError::Invalid("活动节点不存在".into()))?
                .task
                .action
                .clone();
            if let TaskAction::Tool { check, .. } = action
                && state.phase == LoopPhase::Model
            {
                let text = state
                    .context
                    .last()
                    .filter(|message| matches!(message, Message::Tool { .. }))
                    .map(Message::content)
                    .ok_or_else(|| RuntimeError::Invalid("工具节点缺少结果".into()))?
                    .to_owned();
                let passed = state.last_tool_succeeded == Some(true)
                    && match check {
                        ToolCheck::Succeeded => true,
                        ToolCheck::ExitCodeZero => serde_json::from_str::<serde_json::Value>(&text)
                            .ok()
                            .is_some_and(|value| {
                                value["exit_code"].as_i64() == Some(0)
                                    && value.get("success").and_then(|v| v.as_bool()) != Some(false)
                            }),
                    };
                let validation = if state.last_tool_operator {
                    ValidationKind::OperatorVerified
                } else {
                    match check {
                        ToolCheck::Succeeded => ValidationKind::ToolSucceeded,
                        ToolCheck::ExitCodeZero => ValidationKind::ExitCodeZero,
                    }
                };
                finish_node(
                    state,
                    if passed {
                        NodeStatus::Succeeded
                    } else {
                        NodeStatus::Failed
                    },
                    Some(text),
                    Some(validation),
                )?;
                self.commit(state)?;
                return Ok(true);
            }
            match state.phase {
                LoopPhase::Model => self.call_model(state, &mut |_| {}, trace)?,
                LoopPhase::Tools => self.call_tool(state, trace)?,
                _ => return Err(RuntimeError::Invalid("活动图节点执行阶段无效".into())),
            }
            return Ok(true);
        }
        if state.phase != LoopPhase::Model || !state.pending.is_empty() {
            return Ok(false);
        }
        if state.routing.mode == ExecutionMode::Graph {
            let plan = state
                .plans
                .current()
                .ok_or_else(|| RuntimeError::Invalid("缺少图计划".into()))?;
            state
                .graph
                .bind(state.plans.revision(), plan, state.work_revision)?;
        }
        let requested = state.requested_node.take();
        let id = requested.or_else(|| {
            if state.routing.mode == ExecutionMode::Graph {
                state.graph.ready().map(str::to_owned)
            } else {
                state.graph.ready_delegation().map(str::to_owned)
            }
        });
        if let Some(id) = id {
            start_node(state, &id)?;
            self.commit(state)?;
            return Ok(true);
        }
        Ok(false)
    }
}

fn start_node(state: &mut RunState, id: &str) -> Result<(), RuntimeError> {
    let graph = state
        .graph
        .current_mut()
        .ok_or_else(|| RuntimeError::Invalid("尚无图".into()))?;
    let version = graph.plan_version;
    let node = graph
        .nodes
        .get_mut(id)
        .ok_or_else(|| RuntimeError::NotFound(id.into()))?;
    if !matches!(
        node.status,
        NodeStatus::Pending | NodeStatus::Paused | NodeStatus::NeedsCoordinator
    ) {
        return Err(RuntimeError::Invalid("节点不可执行".into()));
    }
    if node.status == NodeStatus::Pending {
        if node.attempts >= 3 {
            return Err(RuntimeError::Invalid("节点尝试次数达到上限".into()));
        }
        node.attempts += 1;
        node.context = Context::new();
        node.pending.clear();
        node.output.clear();
        node.validation = None;
        node.last_tool_succeeded = None;
        node.last_tool_operator = false;
        match &node.task.action {
            TaskAction::Agent { prompt } => {
                node.context.push_user(format!("任务：{}\n指令：{prompt}\n验收条件：{}\n结束时逐条说明结果和证据，明确未验证事项。", node.task.description, node.task.acceptance.join("；")));
                if !node.prior_output.is_empty() {
                    node.context.push_user(format!(
                        "上一版本的结果仅供复核，不代表当前输入仍然有效：{}",
                        node.prior_output
                    ));
                }
                node.phase = LoopPhase::Model;
            }
            TaskAction::Tool {
                name, arguments, ..
            } => {
                node.read_only = state
                    .tools
                    .iter()
                    .any(|tool| tool.name() == name && tool.is_read_only());
                let mut args = Arguments::new();
                for (key, value) in arguments {
                    args = args.with(key, value);
                }
                let call = ToolCall::new(
                    format!("graph:{version}:{id}:{}", node.attempts),
                    name,
                    args,
                );
                node.context
                    .push_assistant_with_tool_calls("执行计划中的工具节点", vec![call.clone()]);
                node.pending.push_back(call);
                node.phase = LoopPhase::Tools;
            }
        }
    }
    node.status = NodeStatus::Running;
    let context = mem::take(&mut node.context);
    state.phase = node.phase.clone();
    state.pending = mem::take(&mut node.pending);
    state.last_tool_succeeded = node.last_tool_succeeded;
    state.last_tool_operator = node.last_tool_operator;
    state.graph.coordinator_context = Some(mem::replace(&mut state.context, context));
    state.graph.active = Some(id.into());
    Ok(())
}

pub(super) fn finish_node(
    state: &mut RunState,
    status: NodeStatus,
    output: Option<String>,
    validation: Option<ValidationKind>,
) -> Result<(), RuntimeError> {
    let id = state
        .graph
        .active
        .take()
        .ok_or_else(|| RuntimeError::Invalid("没有活动节点".into()))?;
    let coordinator = state
        .graph
        .coordinator_context
        .take()
        .ok_or_else(|| RuntimeError::Invalid("缺少协调者上下文".into()))?;
    let graph = state.graph.current_mut().expect("active graph");
    let node = graph.nodes.get_mut(&id).expect("active node");
    node.context = mem::replace(&mut state.context, coordinator);
    node.phase = state.phase.clone();
    node.last_tool_succeeded = state.last_tool_succeeded.take();
    node.last_tool_operator = state.last_tool_operator;
    node.pending = mem::take(&mut state.pending);
    node.status = status;
    node.evidence_revision = state.work_revision;
    if let Some(output) = output {
        node.output = preview(&output, 8192);
        node.validation = validation;
    }
    state.phase = LoopPhase::Model;
    state.status = RunStatus::Running;
    state.result = None;
    Ok(())
}

fn preview(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.into();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[摘要截断；完整结果保留在节点上下文]", &text[..end])
}
