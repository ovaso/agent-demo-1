use super::super::{RunState, RuntimeError};
use super::invocation::{ControlOutput, number, required};
use crate::agent::runtime::coordination;
use crate::tool::{Parameter, ToolCall, ToolDefinition};

pub(in crate::agent::runtime) fn handles(name: &str) -> bool {
    matches!(
        name,
        "runtime_send" | "runtime_ask" | "runtime_reply" | "runtime_wait" | "runtime_inbox"
    )
}

pub(in crate::agent::runtime) fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "runtime_send",
            "发送定向通知，不等待答复。地址为 main 或 node/节点ID，发送者由运行时填写；消息不授予权限。",
            vec![
                Parameter::required("to", "接收地址"),
                Parameter::required("body", "最多2048字节"),
            ],
        ).with_metadata(1789013643, "v1.0.0-20260910"),
        ToolDefinition::new(
            "runtime_ask",
            "发起协作请求。节点默认等待答复并释放执行位置，协调者默认不等待而继续调度；通过 runtime_inbox 读取答复。一次请求在恢复后不会重复投递到上下文。",
            vec![
                Parameter::required("to", "main 或 node/节点ID"),
                Parameter::required("body", "问题，最多2048字节"),
                Parameter::optional("timeout_ms", "1..3600000，默认300000，暂停期间继续计时"),
                Parameter::optional("wait", "true/false，默认节点true、协调者false"),
            ],
        ).with_metadata(1789013643, "v1.0.0-20260910"),
        ToolDefinition::new(
            "runtime_reply",
            "答复收到的请求，必须使用请求ID。只能以实际接收者身份答复；相同答复重投幂等，不同答复不能覆盖。",
            vec![
                Parameter::required("request", "请求ID"),
                Parameter::required("body", "答复或拒绝原因，最多2048字节"),
                Parameter::optional("decline", "拒绝时true，默认false"),
            ],
        ).with_metadata(1789013643, "v1.0.0-20260910"),
        ToolDefinition::new(
            "runtime_wait",
            "节点等待自己已发出的请求；有界等待并检查循环依赖。协调者使用 inbox 继续调度。",
            vec![Parameter::required("request", "自己发出的请求ID")],
        ).with_metadata(1789013643, "v1.0.0-20260910"),
        ToolDefinition::new(
            "runtime_inbox",
            "读取自己收到的消息和请求答复，按变更游标分页。历史保持持久化，读取不增加模型额度。",
            vec![Parameter::optional("after", "变更游标，默认0")],
        ).with_metadata(1789013643, "v1.0.0-20260910"),
    ]
}

pub(in crate::agent::runtime) fn invoke(
    state: &mut RunState,
    call: &ToolCall,
) -> Result<ControlOutput, RuntimeError> {
    let allowed: &[&str] = match call.name() {
        "runtime_send" => &["to", "body"],
        "runtime_ask" => &["to", "body", "timeout_ms", "wait"],
        "runtime_reply" => &["request", "body", "decline"],
        "runtime_wait" => &["request"],
        "runtime_inbox" => &["after"],
        _ => return Err(RuntimeError::Invalid("未知协作工具".into())),
    };
    if call
        .arguments()
        .iter()
        .any(|(name, _)| !allowed.contains(&name))
    {
        return Err(RuntimeError::Invalid("协作工具包含未知参数".into()));
    }
    let now = coordination::messages::now_ms();
    coordination::delivery::tick(state, now)?;
    let mut wait_request = None;
    let mut abort_batch = false;
    let text = match call.name() {
        "runtime_send" | "runtime_ask" => {
            let request = call.name() == "runtime_ask";
            let wait = request && boolean(call, "wait", state.graph.active.is_some())?;
            let timeout = if request {
                Some(
                    call.arguments()
                        .get("timeout_ms")
                        .map(number)
                        .transpose()?
                        .unwrap_or(crate::agent::collaboration::DEFAULT_TIMEOUT_MS),
                )
            } else {
                None
            };
            let id = coordination::messages::send(
                state,
                required(call, "to")?,
                required(call, "body")?,
                timeout,
                wait,
                false,
                now,
            )?;
            if wait {
                wait_request = Some(id.clone());
            }
            serde_json::json!({"message_id":id,"status":"queued"}).to_string()
        }
        "runtime_reply" => {
            coordination::messages::reply(
                state,
                required(call, "request")?,
                required(call, "body")?,
                boolean(call, "decline", false)?,
                false,
                now,
            )?;
            "答复已保存。".into()
        }
        "runtime_wait" => {
            if state.graph.active.is_none() {
                return Err(RuntimeError::Invalid(
                    "协调者应读取 inbox 并继续调度".into(),
                ));
            }
            let id = required(call, "request")?;
            match coordination::delivery::wait_result(state, id)? {
                Some(result) => {
                    abort_batch = !result.succeeded;
                    result.text
                }
                None => {
                    let request = state
                        .collaboration
                        .get(id)
                        .ok_or_else(|| RuntimeError::Invalid(format!("等待请求 {id} 不存在")))?;
                    coordination::messages::check_wait(state, &state.actor(), &request.to)?;
                    wait_request = Some(id.into());
                    String::new()
                }
            }
        }
        "runtime_inbox" => {
            let after = call
                .arguments()
                .get("after")
                .map(number)
                .transpose()?
                .unwrap_or(0);
            let views = coordination::delivery::inbox(state, after, false);
            let cursor = views
                .last()
                .and_then(|value| value["sequence"].as_u64())
                .unwrap_or(after);
            coordination::delivery::mark_seen(state, &views);
            serde_json::json!({"messages":views,"next_cursor":cursor,"latest_sequence":state.collaboration.sequence()}).to_string()
        }
        _ => {
            return Err(RuntimeError::Invalid(format!(
                "未知运行时工具：{}",
                call.name()
            )));
        }
    };
    Ok(ControlOutput {
        abort_batch,
        text,
        plan_ready: false,
        wait_request,
    })
}

fn boolean(call: &ToolCall, name: &str, default: bool) -> Result<bool, RuntimeError> {
    call.arguments()
        .get(name)
        .map(|value| {
            value
                .parse()
                .map_err(|_| RuntimeError::Invalid(format!("{name} 必须为 true 或 false")))
        })
        .unwrap_or(Ok(default))
}
