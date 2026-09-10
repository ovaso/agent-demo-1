use super::{content::Content, response::parse_response, usage};
use agent_core::model::{ModelError, ModelResponse};
use serde_json::{Value, json};
use std::io::BufRead;

pub(super) fn parse_stream(
    response: impl BufRead,
    on_text_delta: &mut dyn FnMut(&str),
) -> Result<ModelResponse, ModelError> {
    let mut content = Content::default();
    let mut text = String::new();
    let mut usage = usage::Usage::default();
    let mut model = None;
    let mut stop = None;
    let mut finished = false;
    for line in response
        .take(super::super::continuation::MAX_RESPONSE_BYTES)
        .lines()
    {
        let line = line.map_err(ModelError::new)?;
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        let event: Value = serde_json::from_str(data).map_err(ModelError::new)?;
        match event["type"].as_str() {
            Some("message_start") => {
                usage.update(&event["message"]);
                model = event
                    .pointer("/message/model")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            Some("message_delta") => {
                usage.update(&event);
                if let Some(reason) = event.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    stop = Some(reason.to_owned());
                }
            }
            Some("error") => return Err(ModelError::new(event.get("error").unwrap_or(&event))),
            Some("content_block_start") => {
                let index = event["index"].as_u64().unwrap_or(0) as usize;
                let block = event
                    .get("content_block")
                    .ok_or_else(|| ModelError::new("Anthropic 流式事件缺少内容块"))?;
                content.start(index, block)?;
                if block["type"] == "text"
                    && let Some(part) = block["text"].as_str()
                {
                    text.push_str(part);
                    on_text_delta(part);
                }
            }
            Some("content_block_delta") => {
                let index = event["index"].as_u64().unwrap_or(0) as usize;
                let delta = event
                    .get("delta")
                    .ok_or_else(|| ModelError::new("Anthropic 流式事件缺少增量"))?;
                content.delta(index, delta)?;
                if delta["type"] == "text_delta"
                    && let Some(part) = delta["text"].as_str()
                {
                    text.push_str(part);
                    on_text_delta(part);
                }
            }
            Some("message_stop") => {
                finished = true;
                break;
            }
            _ => {}
        }
    }
    if !finished {
        return Err(ModelError::new("anthropic 流在结束标记之前中断"));
    }
    let complete = super::super::stop_reason::anthropic(stop.as_deref()).is_complete();
    let value = json!({"content":content.finish(complete)?,"stop_reason":stop,"model":model});
    let parsed = parse_response(&value)?.with_usage(usage.finish());
    Ok(if text.is_empty() {
        parsed
    } else {
        parsed.with_text(text)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merges_cumulative_usage_without_double_counting() {
        let events = [
            json!({"type":"message_start", "message":{"usage":{"input_tokens":10,"output_tokens":1,"cache_read_input_tokens":30,"cache_creation_input_tokens":20}}}),
            json!({"type":"content_block_delta", "delta":{"type":"text_delta","text":"Hello"}}),
            json!({"type":"message_delta", "usage":{"output_tokens":3}}),
            json!({"type":"message_delta", "usage":{"input_tokens":12,"output_tokens":5}}),
            json!({"type":"message_stop"}),
        ];
        let stream = events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>();
        let mut text = String::new();
        let response = parse_stream(stream.as_bytes(), &mut |delta| text.push_str(delta)).unwrap();
        assert_eq!(text, "Hello");
        assert_eq!(response.usage().input_tokens, Some(62));
        assert_eq!(response.usage().output_tokens, Some(5));
        assert_eq!(response.usage().total_tokens(), Some(67));
        assert_eq!(response.usage().cache_write_input_tokens, Some(20));
    }

    #[test]
    fn missing_usage_is_unknown_and_errors_or_truncation_fail() {
        let response = parse_stream(
            b"data: {\"type\":\"message_stop\"}\n".as_slice(),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(response.usage().total_tokens(), None);
        assert_eq!(response.usage().cache_hit(), None);
        assert!(
            parse_stream(
                b"data: {\"type\":\"message_start\"}\n".as_slice(),
                &mut |_| {}
            )
            .is_err()
        );
        assert!(
            parse_stream(
                b"data: {\"type\":\"error\",\"error\":{\"message\":\"failed\"}}\n".as_slice(),
                &mut |_| {}
            )
            .is_err()
        );
    }
}
