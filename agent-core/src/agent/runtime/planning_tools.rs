use super::super::{
    blackboard::{BoardUpdate, EntryKind},
    planning::Plan,
};
use super::{RunState, RuntimeError, WorkIntent};
use crate::tool::{Parameter, ToolCall, ToolDefinition};

pub(super) struct ControlOutput {
    pub text: String,
    pub plan_ready: bool,
}

pub(super) fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "runtime_plan",
            "提交或修订结构化计划。计划 JSON 含 goal、requirements 字符串数组、tasks 数组。每个任务含 id、description、depends_on、acceptance、action。action 为 {\"kind\":\"agent\",\"prompt\":\"...\"} 或 {\"kind\":\"tool\",\"name\":\"...\",\"arguments\":{\"参数\":\"值\"},\"check\":\"succeeded\"/\"exit_code_zero\"}。goal 由运行时固定为原任务；旧验收要求必须保留。",
            vec![
                Parameter::required("expected_revision", "首次为 0，修订时为当前计划版本"),
                Parameter::required("plan", "计划 JSON 字符串"),
            ],
        ),
        ToolDefinition::new(
            "runtime_board_write",
            "写入带版本的共享记录，不能将模型意见标记为程序验证。update JSON 含 key、expected_revision（首次 0）、kind（observation/hypothesis/decision/blocker/artifact）、content、sources（可选，每项 uri 和 version）。作者由运行时指定。",
            vec![Parameter::required("update", "共享记录更新 JSON 字符串")],
        ),
        ToolDefinition::new(
            "runtime_board_read",
            "读取共享记录。指定 key 可查询一条，revision 可读历史版本；否则 after 为变更游标，返回有界的最新记录及 next_cursor。",
            vec![
                Parameter::optional("key", "记录标识"),
                Parameter::optional("revision", "指定记录的历史版本"),
                Parameter::optional("after", "变更游标，默认 0"),
            ],
        ),
        ToolDefinition::new(
            "runtime_plan_ready",
            "只规划模式下交付当前已保存计划，暂停等待用户执行。调用后的本批其余工具不执行。",
            vec![],
        ),
    ]
}

pub(super) fn invoke(state: &mut RunState, call: &ToolCall) -> Result<ControlOutput, RuntimeError> {
    let allowed: &[&str] = match call.name() {
        "runtime_plan" => &["expected_revision", "plan"],
        "runtime_board_write" => &["update"],
        "runtime_board_read" => &["key", "revision", "after"],
        "runtime_plan_ready" => &[],
        _ => return Err(RuntimeError::Invalid("未知运行时工具".into())),
    };
    if call
        .arguments()
        .iter()
        .any(|(name, _)| !allowed.contains(&name))
    {
        return Err(RuntimeError::Invalid("运行时工具包含未知参数".into()));
    }
    let text = match call.name() {
        "runtime_plan" => {
            let source = required(call, "plan")?;
            if source.len() > 128 * 1024 {
                return Err(RuntimeError::Invalid("计划输入超过 128 KiB".into()));
            }
            let mut plan: Plan = serde_json::from_str(source)
                .map_err(|error| RuntimeError::Invalid(error.to_string()))?;
            plan.goal.clone_from(&state.goal);
            for task in &plan.tasks {
                if let super::super::planning::TaskAction::Tool { name, .. } = &task.action
                    && !state.tools.iter().any(|tool| tool.name() == name)
                {
                    return Err(RuntimeError::Invalid(format!("计划引用未注册工具：{name}")));
                }
            }
            let revision = state
                .plans
                .propose(number(required(call, "expected_revision")?)?, plan)?;
            serde_json::json!({"plan_version": revision}).to_string()
        }
        "runtime_board_write" => {
            let source = required(call, "update")?;
            if source.len() > 16 * 1024 {
                return Err(RuntimeError::Invalid("共享记录输入过大".into()));
            }
            let update: BoardUpdate = serde_json::from_str(source)
                .map_err(|error| RuntimeError::Invalid(error.to_string()))?;
            if update.kind == EntryKind::Verification {
                return Err(RuntimeError::Invalid("模型不能伪造程序验证记录".into()));
            }
            let entry = state
                .blackboard
                .write("main", state.plans.revision(), update)?;
            serde_json::json!({"key":entry.key,"revision":entry.revision,"sequence":entry.sequence})
                .to_string()
        }
        "runtime_board_read" => read_board(state, call)?,
        "runtime_plan_ready" => {
            if state.intent != WorkIntent::PlanOnly || state.plans.current().is_none() {
                return Err(RuntimeError::Invalid(
                    "当前不是具备有效计划的只规划任务".into(),
                ));
            }
            "计划已保存，等待执行指令。".into()
        }
        _ => unreachable!(),
    };
    Ok(ControlOutput {
        text,
        plan_ready: call.name() == "runtime_plan_ready",
    })
}

fn read_board(state: &RunState, call: &ToolCall) -> Result<String, RuntimeError> {
    let key = call.arguments().get("key");
    if key.is_none() && call.arguments().get("revision").is_some() {
        return Err(RuntimeError::Invalid("读取历史版本需要 key".into()));
    }
    let after = call
        .arguments()
        .get("after")
        .map(number)
        .transpose()?
        .unwrap_or(0);
    let candidates = if let Some(key) = key {
        let entry = match call.arguments().get("revision") {
            Some(value) => state.blackboard.version(key, number(value)?),
            None => state.blackboard.latest(key),
        };
        entry.into_iter().collect()
    } else {
        state.blackboard.changes(after, 32)
    };
    let mut bytes = 256;
    let mut entries = Vec::new();
    let mut next_cursor = after;
    for entry in candidates {
        let size = serde_json::to_vec(entry)
            .map_err(RuntimeError::storage)?
            .len()
            + 1;
        if bytes + size > state.limits.max_tool_output_bytes {
            break;
        }
        bytes += size;
        next_cursor = next_cursor.max(entry.sequence);
        entries.push(entry);
    }
    Ok(serde_json::json!({"entries":entries,"next_cursor":next_cursor,"latest_sequence":state.blackboard.sequence()}).to_string())
}

pub(super) fn required<'a>(call: &'a ToolCall, name: &str) -> Result<&'a str, RuntimeError> {
    call.arguments()
        .get(name)
        .ok_or_else(|| RuntimeError::Invalid(format!("缺少参数 {name}")))
}

pub(super) fn number(value: &str) -> Result<u64, RuntimeError> {
    value
        .parse()
        .map_err(|_| RuntimeError::Invalid("参数必须为非负整数".into()))
}
