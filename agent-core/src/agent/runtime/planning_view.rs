//! Borrowed, role-scoped runtime data sent to the model. No prompt policy here.
use super::{RunState, RuntimeError, WorkIntent};
use crate::{
    agent::{
        delegation::AgentPolicy,
        graph::{NodeStatus, ValidationKind},
        planning::Plan,
        routing::ExecutionMode,
    },
    tool::{Parameter, ToolDefinition},
};
use serde::Serialize;

#[derive(Serialize)]
struct Overview<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    token_budget: Option<TokenBudgetView>,
    active_node: Option<&'a str>,
    actor: &'a str,
    agent_budget: Option<&'a AgentPolicy>,
    board_sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    execution_tool_definitions: Option<Vec<ToolView<'a>>>,
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

// Source timestamps and reference versions are persisted locally, not repeated
// in the model prompt. Version-only edits must not change its tool schema text.
#[derive(Serialize)]
struct ToolView<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a [Parameter],
    read_only: bool,
}

impl<'a> From<&'a ToolDefinition> for ToolView<'a> {
    fn from(tool: &'a ToolDefinition) -> Self {
        Self {
            name: tool.name(),
            description: tool.description(),
            parameters: tool.parameters(),
            read_only: tool.is_read_only(),
        }
    }
}

#[derive(Serialize)]
struct TokenBudgetView {
    limit: u64,
    used: u64,
    remaining: u64,
    max_output: Option<u64>,
    estimated: u64,
    unmetered_requests: u64,
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

pub(super) fn value(state: &RunState) -> Result<serde_json::Value, RuntimeError> {
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
    let usage = state.budget.token_usage();
    let overview = Overview {
        token_budget: state.limits.max_total_tokens.map(|limit| TokenBudgetView {
            limit,
            used: usage.total_tokens(),
            remaining: limit.saturating_sub(usage.total_tokens()),
            max_output: state.limits.max_output_tokens,
            estimated: usage.estimated_tokens(),
            unmetered_requests: usage.unmetered_requests(),
        }),
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
                    .map(ToolView::from)
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
    serde_json::to_value(overview).map_err(RuntimeError::storage)
}

#[cfg(test)]
pub(super) fn message(state: &RunState) -> Result<crate::context::Message, RuntimeError> {
    super::serialization::encode_prefixed(
        &value(state)?,
        super::prompt_history::SNAPSHOT_PREFIX,
        state.limits.max_context_bytes,
    )
    .map(crate::context::Message::user)
}

fn preview(text: &str) -> &str {
    let mut end = text.len().min(512);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
