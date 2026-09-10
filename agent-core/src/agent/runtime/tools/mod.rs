//! 运行时控制工具的定义与分发；各工具组共享调用协议。
mod agents;
mod invocation;
mod messages;
mod planning;

use super::{RunState, RuntimeError};
use crate::tool::{Registry, ToolCall, ToolDefinition};
use invocation::ControlOutput;

pub(super) fn definitions() -> Vec<ToolDefinition> {
    let mut tools = planning::definitions();
    tools.extend(agents::definitions());
    tools.extend(messages::definitions());
    tools
}

pub(super) fn invoke(
    state: &mut RunState,
    call: &ToolCall,
    tools: &Registry,
) -> Result<ControlOutput, RuntimeError> {
    if messages::handles(call.name()) {
        messages::invoke(state, call)
    } else if agents::handles(call.name()) {
        agents::invoke(state, call)
    } else {
        planning::invoke(state, call, tools)
    }
}
