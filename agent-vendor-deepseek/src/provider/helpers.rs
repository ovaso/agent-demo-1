use agent_core::{model::ModelError, tool::Arguments};
use serde_json::Value;
pub(super) fn arguments_json(arguments: &Arguments) -> String {
    Value::Object(
        arguments
            .iter()
            .map(|(name, value)| (name.to_owned(), Value::String(value.to_owned())))
            .collect(),
    )
    .to_string()
}

pub(super) fn parse_arguments(arguments: &str) -> Result<Arguments, ModelError> {
    let value: Value = serde_json::from_str(arguments).map_err(ModelError::new)?;
    let object = value
        .as_object()
        .ok_or_else(|| ModelError::new("工具参数必须是 JSON 对象"))?;

    Ok(object
        .iter()
        .fold(Arguments::new(), |arguments, (name, value)| {
            arguments.with(
                name,
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            )
        }))
}
