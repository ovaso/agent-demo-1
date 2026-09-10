//! Shared runtime helpers for generated tools; not a separate tool authoring API.
use serde::{Serialize, de::DeserializeOwned};

use super::{Arguments, ToolError, ToolOutput};

pub fn text_argument<'a>(arguments: &'a Arguments, name: &str) -> Result<&'a str, ToolError> {
    arguments
        .get(name)
        .ok_or_else(|| ToolError::new(format!("missing argument `{name}`")))
}

pub fn argument<T: DeserializeOwned>(
    arguments: &Arguments,
    name: &str,
    optional: bool,
) -> Result<T, ToolError> {
    match arguments.get(name) {
        Some(value) => serde_json::from_str(value)
            .or_else(|_| serde_json::from_value(serde_json::Value::String(value.to_owned())))
            .map_err(|error| ToolError::new(format!("invalid argument `{name}`: {error}"))),
        None if optional => serde_json::from_value(serde_json::Value::Null)
            .map_err(|error| ToolError::new(error.to_string())),
        None => Err(ToolError::new(format!("missing argument `{name}`"))),
    }
}

pub fn json_output<T: Serialize>(value: &T, finish: bool) -> Result<ToolOutput, ToolError> {
    let content =
        serde_json::to_string(value).map_err(|error| ToolError::new(error.to_string()))?;
    Ok(if finish {
        ToolOutput::finish_session(
            serde_json::from_str::<serde_json::Value>(&content)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or(content),
        )
    } else {
        ToolOutput::text(content)
    })
}
