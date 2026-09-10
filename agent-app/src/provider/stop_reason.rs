use agent_core::model::StopReason;

pub(super) fn openai(reason: Option<&str>) -> StopReason {
    match reason {
        None | Some("stop" | "tool_calls" | "function_call") => StopReason::Complete,
        Some("length") => StopReason::Length,
        Some("content_filter" | "refusal") => StopReason::Refused,
        Some(other) => StopReason::Other(other.into()),
    }
}

pub(super) fn anthropic(reason: Option<&str>) -> StopReason {
    match reason {
        None | Some("end_turn" | "tool_use" | "stop_sequence") => StopReason::Complete,
        Some("max_tokens" | "model_context_window_exceeded") => StopReason::Length,
        Some("refusal") => StopReason::Refused,
        Some(other) => StopReason::Other(other.into()),
    }
}
