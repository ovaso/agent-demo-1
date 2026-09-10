use crate::{agent::runtime::RuntimeError, tool::ToolCall};

pub(in crate::agent::runtime) struct ControlOutput {
    pub(in crate::agent::runtime) abort_batch: bool,
    pub(in crate::agent::runtime) wait_request: Option<String>,
    pub(in crate::agent::runtime) text: String,
    pub(in crate::agent::runtime) plan_ready: bool,
}

pub(in crate::agent::runtime) fn required<'a>(
    call: &'a ToolCall,
    name: &str,
) -> Result<&'a str, RuntimeError> {
    call.arguments()
        .get(name)
        .ok_or_else(|| RuntimeError::Invalid(format!("缺少参数 {name}")))
}

pub(in crate::agent::runtime) fn number(value: &str) -> Result<u64, RuntimeError> {
    value
        .parse()
        .map_err(|_| RuntimeError::Invalid("参数必须为非负整数".into()))
}
