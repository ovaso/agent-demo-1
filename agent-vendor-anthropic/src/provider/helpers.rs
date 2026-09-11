use agent_core::{model::ModelError, tool::Arguments};
use serde_json::{Map, Value};
pub(super) fn required_string<'a>(
    value: &'a Value,
    field: &str,
    message: &str,
) -> Result<&'a str, ModelError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| ModelError::new(message))
}

pub(super) fn arguments_value(arguments: &Arguments) -> Value {
    Value::Object(
        arguments
            .iter()
            .map(|(name, value)| (name.to_owned(), Value::String(value.to_owned())))
            .collect(),
    )
}

pub(super) fn arguments_from_object(object: &Map<String, Value>) -> Arguments {
    object
        .iter()
        .fold(Arguments::new(), |arguments, (name, value)| {
            arguments.with(
                name,
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            )
        })
}
