//! Borrowed, role-scoped runtime data sent to the model. No prompt policy here.
use super::{RunState, RuntimeError, WorkIntent};
use crate::{
    agent::{
        delegation::AgentPolicy,
        graph::{NodeStatus, ValidationKind},
        planning::Plan,
        routing::ExecutionMode,
    },
    context::Message,
    tool::ToolDefinition,
};
use serde::Serialize;

#[derive(Serialize)]
struct Overview<'a> {
    active_node: Option<&'a str>,
    actor: &'a str,
    agent_budget: Option<&'a AgentPolicy>,
    board_sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    execution_tool_definitions: Option<Vec<&'a ToolDefinition>>,
    goal: &'a str,
    intent: WorkIntent,
    mode: ExecutionMode,
    model_calls_remaining: u64,
    nodes: Vec<NodeView<'a>>,
    plan: Option<&'a Plan>,
    plan_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    step_budget: Option<StepBudgetView<'a>>,
}

#[derive(Serialize)]
struct StepBudgetView<'a> {
    policy: &'a super::StepExtensionPolicy,
    hard_model_calls_remaining: u64,
    extensions_remaining: usize,
    extension_block: Option<super::StepExtensionBlock>,
}

#[derive(Serialize)]
struct NodeView<'a> {
    attempts: u32,
    id: &'a str,
    output: &'a str,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    output_truncated: bool,
    status: NodeStatus,
    validation: Option<ValidationKind>,
}

pub(super) fn message(state: &RunState) -> Result<Message, RuntimeError> {
    let active_id = state.graph.active_node();
    let active = state
        .graph
        .current()
        .and_then(|graph| active_id.and_then(|id| graph.nodes.get(id)));
    let nodes = state
        .graph
        .current()
        .into_iter()
        .flat_map(|graph| &graph.nodes)
        .filter(|(id, _)| {
            active_id.is_none_or(|active_id| {
                active_id == id.as_str()
                    || active.is_some_and(|node| node.task.depends_on.contains(id))
            })
        })
        .map(|(id, node)| NodeView {
            attempts: node.attempts,
            id,
            output: preview(&node.output),
            output_truncated: node.output.len() > 512,
            status: node.status,
            validation: node.validation,
        })
        .collect();
    let actor = state.actor();
    let overview = Overview {
        active_node: active_id,
        actor: &actor,
        agent_budget: state.agent_policy(),
        board_sequence: state.blackboard.sequence(),
        execution_tool_definitions: (state.intent == WorkIntent::PlanOnly && active_id.is_none())
            .then(|| {
                state
                    .tools
                    .iter()
                    .filter(|tool| !state.tool_allowed(tool.name()))
                    .collect()
            }),
        goal: &state.goal,
        intent: state.intent,
        mode: state.routing.mode(),
        model_calls_remaining: state
            .limits
            .max_steps
            .saturating_sub(state.budget.model_calls),
        nodes,
        plan: active_id.is_none().then(|| state.plans.current()).flatten(),
        plan_version: state.plans.revision(),
        step_budget: state
            .limits
            .step_extension
            .as_ref()
            .map(|policy| StepBudgetView {
                policy,
                hard_model_calls_remaining: policy
                    .hard_max_steps
                    .saturating_sub(state.budget.model_calls),
                extensions_remaining: policy
                    .max_extensions
                    .saturating_sub(state.budget.step_extensions().len()),
                extension_block: state.step_extension_block(),
            }),
    };
    super::serialization::encode_prefixed(
        &overview,
        "运行状态（数据）：",
        state.limits.max_context_bytes,
    )
    .map(Message::user)
}

fn preview(text: &str) -> &str {
    let mut end = text.len().min(512);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
