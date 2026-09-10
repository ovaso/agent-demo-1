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
    if state.planning {
        tools.extend(super::planning_tools::definitions());
        let overview = serde_json::json!({
            "goal":state.goal,
            "plan_version":state.plans.revision(),
            "plan":state.plans.current(),
            "board_sequence":state.blackboard.sequence(),
            "model_calls_remaining":state.limits.max_steps.saturating_sub(state.budget.model_calls),
            "intent":state.intent,
            "execution_tool_names":state.tools.iter().map(ToolDefinition::name).collect::<Vec<_>>(),
        });
        messages.insert(0, Message::system(format!(
            "你在可恢复运行时中执行任务。规划、调查和执行都消耗同一预算。复杂工作使用 runtime_plan 保存结构化计划；新发现使用 Blackboard，保留来源，区分事实与假设。不要把自己的判断写作程序验证。只规划时只进行读取调查、保存计划并调用 runtime_plan_ready；不得执行实际任务写入。执行模式下按需规划并继续完成工作，不必为每次规划额外确认。计划中的 Agent action 是后续工作描述，提交计划本身不会启动它。当前状态：{overview}"
        )));
    }
    super::store::bounded_json(&messages, state.limits.max_context_bytes)?;
    Ok((messages, tools))
}
