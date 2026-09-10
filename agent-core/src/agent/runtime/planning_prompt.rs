use super::{RunState, RuntimeError, WorkIntent};
use crate::{context::Message, tool::ToolDefinition};

pub(super) fn request_context(
    state: &RunState,
) -> Result<(Vec<Message>, Vec<ToolDefinition>), RuntimeError> {
    let mut messages = state.context.snapshot();
    let mut tools: Vec<_> = state
        .tools
        .iter()
        .filter(|tool| state.intent != WorkIntent::PlanOnly || tool.is_read_only())
        .cloned()
        .collect();
    if state.planning || state.graph.current().is_some() {
        if state.planning {
            tools.extend(super::planning_tools::definitions());
        }
        let overview = serde_json::json!({
            "goal":state.goal,
            "plan_version":state.plans.revision(),
            "plan":state.plans.current(),
            "board_sequence":state.blackboard.sequence(),
            "model_calls_remaining":state.limits.max_steps.saturating_sub(state.budget.model_calls),
            "intent":state.intent,
            "mode":state.routing.mode(),
            "active_node":state.graph.active_node(),
            "nodes":state.graph.current().map(|graph| graph.nodes.iter().map(|(id,node)| serde_json::json!({"id":id,"status":node.status,"output":node.output,"validation":node.validation,"attempts":node.attempts})).collect::<Vec<_>>()),
            "execution_tool_definitions":state.tools,
        });
        messages.insert(0, Message::system(format!(
            "你在可恢复运行时中执行任务。规划、路由和执行消耗同一预算。复杂工作使用 runtime_plan 保存计划，用 runtime_route 选择 loop 或 graph，可途中切换。Graph 自动按依赖调度；Loop 可用 runtime_run_node 继续剩余节点。提交计划本身不启动节点。节点失败时协调者重试、修复或重新规划，不可直接宣告完成。新发现写入 Blackboard 并保留来源。ModelReported 仅为节点模型报告，不能说成程序验证；确定性检查使用带 exit_code_zero 的工具节点。只规划时只调查和提交计划，不启动图执行。节点内需改变整体方案时，可切回 loop 交给协调者。当前状态：{overview}"
        )));
    }
    super::store::bounded_json(&messages, state.limits.max_context_bytes)?;
    Ok((messages, tools))
}
