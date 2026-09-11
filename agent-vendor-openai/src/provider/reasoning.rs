use agent_core::model::ModelError;
use serde_json::{Value, json};

pub(super) fn validate_effort(value: Option<&str>) -> Result<(), ModelError> {
    if value.is_some_and(|v| {
        !matches!(
            v,
            "default" | "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
        )
    }) {
        return Err(ModelError::new("推理强度取值不支持"));
    }
    Ok(())
}
pub(super) fn apply(body: &mut Value, effort: Option<&str>) {
    let Some(effort) = effort.filter(|v| *v != "default") else {
        return;
    };
    body["reasoning_effort"] = json!(effort);
}
