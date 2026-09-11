use agent_core::model::StopReason;

pub(super) fn parse(reason: Option<&str>) -> StopReason {
    match reason {
        None | Some("stop" | "tool_calls" | "function_call") => StopReason::Complete,
        Some("length") => StopReason::Length,
        Some("content_filter" | "refusal") => StopReason::Refused,
        Some(other) => StopReason::Other(other.into()),
    }
}
