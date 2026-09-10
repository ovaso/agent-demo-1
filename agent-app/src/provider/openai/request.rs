use super::helpers::arguments_json;
use agent_core::{context::Message, model::ModelError, tool::ToolDefinition};
use serde_json::{Map, Value, json};
pub(super) fn messages(
    messages: &[Message],
    memories: &[agent_core::memory::Memory],
) -> Result<Vec<Value>, ModelError> {
    let mut output = Vec::new();
    let memory_text = memories
        .iter()
        .map(|memory| format!("[{}] {}", memory.id(), memory.content()))
        .collect::<Vec<_>>()
        .join("\n");

    if !memory_text.is_empty() {
        output.push(json!({
            "role": "system",
            "content": format!("可参考以下长期记忆：\n{memory_text}"),
        }));
    }

    for message in messages {
        match message {
            Message::System { content } => {
                output.push(json!({"role": "system", "content": content}))
            }
            Message::User { content } => output.push(json!({"role": "user", "content": content})),
            Message::Assistant {
                content,
                tool_calls,
                continuation,
            } => {
                let mut value = json!({"role": "assistant", "content": content});
                if !tool_calls.is_empty() {
                    value["tool_calls"] = Value::Array(
                        tool_calls
                            .iter()
                            .map(|call| {
                                json!({
                                    "id": call.id(),
                                    "type": "function",
                                    "function": {
                                        "name": call.name(),
                                        "arguments": arguments_json(call.arguments()),
                                    }
                                })
                            })
                            .collect(),
                    );
                }
                if let Some(continuation) = continuation {
                    if continuation.protocol() != super::super::continuation::OPENAI {
                        return Err(ModelError::new("模型续接协议不匹配"));
                    }
                    if let Some(reasoning) = continuation.data().get("reasoning_content") {
                        value["reasoning_content"] = reasoning.clone();
                    }
                }
                output.push(value);
            }
            Message::Tool {
                call_id, content, ..
            } => output.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": content,
            })),
        }
    }

    Ok(output)
}

pub(super) fn tools(definitions: &[ToolDefinition]) -> Vec<Value> {
    definitions
        .iter()
        .map(|definition| {
            let required = definition
                .parameters()
                .iter()
                .filter(|parameter| parameter.is_required())
                .map(|parameter| Value::String(parameter.name().to_owned()))
                .collect::<Vec<_>>();
            let properties = definition
                .parameters()
                .iter()
                .map(|parameter| {
                    (
                        parameter.name().to_owned(),
                        json!({
                            "type": "string",
                            "description": parameter.description(),
                        }),
                    )
                })
                .collect::<Map<String, Value>>();
            json!({
                "type": "function",
                "function": {
                    "name": definition.name(),
                    "description": definition.description(),
                    "parameters": {
                        "type": "object",
                        "properties": properties,
                        "required": required,
                        "additionalProperties": false,
                    }
                }
            })
        })
        .collect()
}
