use super::planning_tools::{ControlOutput, number, required};
use super::{RunState, RuntimeError};
use crate::tool::{Parameter, ToolCall, ToolDefinition};

pub(super) fn handles(name: &str) -> bool {
    matches!(
        name,
        "runtime_delegate"
            | "runtime_agents"
            | "runtime_agent_budget"
            | "runtime_cancel_agent"
            | "runtime_result"
    )
}

pub(super) fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "runtime_delegate",
            "协调者创建独立 Agent，批次结算后顺序调度；支持 Loop 模式，无需先创建计划图。spec JSON 含 name、instruction、acceptance 字符串数组，tools 可选工具名数组，max_steps 默认4且不超过根总额度，depends_on 可选工作节点ID数组。子 Agent 不能递归创建其他 Agent。",
            vec![Parameter::required("spec", "委托 JSON 字符串")],
        ),
        ToolDefinition::new(
            "runtime_agents",
            "列出本任务工作节点、联系地址、状态、局部预算与结果摘要。",
            vec![],
        ),
        ToolDefinition::new(
            "runtime_agent_budget",
            "协调者调整子 Agent 的累计模型步数上限；不清零用量，不增加根任务额度。",
            vec![
                Parameter::required("node", "工作节点 ID"),
                Parameter::required("max_steps", "新的累计局部上限"),
            ],
        ),
        ToolDefinition::new(
            "runtime_cancel_agent",
            "协调者取消不再需要的子任务，保留结果和用量。",
            vec![Parameter::required("node", "工作节点 ID")],
        ),
        ToolDefinition::new(
            "runtime_result",
            "读取本任务节点的有界结果片段，包括历史计划版本，避免共享整个对话。",
            vec![
                Parameter::required("node", "节点 ID"),
                Parameter::optional("version", "计划版本，默认当前"),
                Parameter::optional("offset", "结果字节偏移，默认0"),
            ],
        ),
    ]
}

pub(super) fn invoke(state: &mut RunState, call: &ToolCall) -> Result<ControlOutput, RuntimeError> {
    let allowed: &[&str] = match call.name() {
        "runtime_delegate" => &["spec"],
        "runtime_agents" => &[],
        "runtime_agent_budget" => &["node", "max_steps"],
        "runtime_cancel_agent" => &["node"],
        "runtime_result" => &["node", "version", "offset"],
        _ => return Err(RuntimeError::Invalid("未知 Agent 工具".into())),
    };
    if call
        .arguments()
        .iter()
        .any(|(name, _)| !allowed.contains(&name))
    {
        return Err(RuntimeError::Invalid("Agent 工具包含未知参数".into()));
    }
    let text = match call.name() {
        "runtime_delegate" => {
            let source = required(call, "spec")?;
            if source.len() > 32 * 1024 {
                return Err(RuntimeError::Invalid("委托说明过大".into()));
            }
            let spec: super::super::delegation::AgentSpec = serde_json::from_str(source)
                .map_err(|error| RuntimeError::Invalid(error.to_string()))?;
            let id = spec.name.clone();
            super::delegation::create(state, spec)?;
            serde_json::json!({"node":id,"address":format!("node/{id}"),"status":"queued"})
                .to_string()
        }
        "runtime_agents" => {
            let nodes: Vec<_> = state.graph.current().into_iter().flat_map(|run| &run.nodes).map(|(id,node)| serde_json::json!({"node":id,"address":format!("node/{id}"),"status":node.status,"budget":node.policy,"summary":node.task.description})).collect();
            let text = serde_json::to_string(&nodes).map_err(RuntimeError::storage)?;
            if text.len() > state.limits.max_tool_output_bytes {
                return Err(RuntimeError::Invalid("节点列表超过结果上限".into()));
            }
            text
        }
        "runtime_agent_budget" => {
            super::delegation::budget(
                state,
                required(call, "node")?,
                number(required(call, "max_steps")?)?,
                false,
            )?;
            "局部额度已调整，根额度和已消耗用量保持不变。".into()
        }
        "runtime_cancel_agent" => {
            super::delegation::cancel(state, required(call, "node")?, false)?;
            "任务已取消，历史与用量保留。".into()
        }
        "runtime_result" => result(state, call)?,
        _ => unreachable!(),
    };
    Ok(ControlOutput {
        text,
        plan_ready: false,
    })
}

fn result(state: &RunState, call: &ToolCall) -> Result<String, RuntimeError> {
    let id = required(call, "node")?;
    let version = call
        .arguments()
        .get("version")
        .map(number)
        .transpose()?
        .unwrap_or_else(|| state.graph.current().map_or(0, |run| run.plan_version));
    let run = state
        .graph
        .history()
        .iter()
        .find(|run| run.plan_version == version)
        .ok_or_else(|| RuntimeError::NotFound("计划版本".into()))?;
    let node = run
        .nodes
        .get(id)
        .ok_or_else(|| RuntimeError::NotFound(id.into()))?;
    if !matches!(
        node.status,
        super::super::graph::NodeStatus::Succeeded
            | super::super::graph::NodeStatus::Failed
            | super::super::graph::NodeStatus::Cancelled
    ) {
        return Err(RuntimeError::Invalid("节点尚未返回最终结果".into()));
    }
    let content = state
        .node_context(version, id)
        .and_then(|context| context.last())
        .map_or(node.output.as_str(), |message| message.content());
    let offset = call
        .arguments()
        .get("offset")
        .map(number)
        .transpose()?
        .unwrap_or(0);
    let offset = usize::try_from(offset).map_err(|_| RuntimeError::Invalid("偏移过大".into()))?;
    if offset > content.len() || !content.is_char_boundary(offset) {
        return Err(RuntimeError::Invalid(
            "偏移需位于结果中的 UTF-8 边界".into(),
        ));
    }
    let mut end = (offset + (state.limits.max_tool_output_bytes / 8).min(8192)).min(content.len());
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    Ok(serde_json::json!({"node":id,"version":version,"status":node.status,"validation":node.validation,"text":&content[offset..end],"next_offset":end,"truncated":end<content.len()}).to_string())
}
