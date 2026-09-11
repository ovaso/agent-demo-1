use super::{content::Content, response::parse_response, usage};
use agent_core::model::{ModelError, ModelResponse, ModelStreamEvent};
use serde_json::{Value, json};
use std::io::BufRead;

pub(super) fn parse_stream(
    response: impl BufRead,
    on_event: &mut dyn FnMut(ModelStreamEvent<'_>),
) -> Result<ModelResponse, ModelError> {
    let mut content = Content::default();
    let mut text = String::new();
    let mut usage = usage::Usage::default();
    let mut model = None;
    let mut stop = None;
    let mut finished = false;
    for line in response
        .take(agent_vendor::http::MAX_RESPONSE_BYTES)
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
                if block["type"] == "thinking"
                    && let Some(part) = block["thinking"].as_str()
                {
                    on_event(ModelStreamEvent::ReasoningDelta(part));
                }
                if block["type"] == "text"
                    && let Some(part) = block["text"].as_str()
                {
                    text.push_str(part);
                    on_event(ModelStreamEvent::TextDelta(part));
                }
            }
            Some("content_block_delta") => {
                let index = event["index"].as_u64().unwrap_or(0) as usize;
                let delta = event
                    .get("delta")
                    .ok_or_else(|| ModelError::new("Anthropic 流式事件缺少增量"))?;
                content.delta(index, delta)?;
                if delta["type"] == "thinking_delta"
                    && let Some(part) = delta["thinking"].as_str()
                {
                    on_event(ModelStreamEvent::ReasoningDelta(part));
                }
                if delta["type"] == "text_delta"
                    && let Some(part) = delta["text"].as_str()
                {
                    text.push_str(part);
                    on_event(ModelStreamEvent::TextDelta(part));
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
    let complete = super::stop_reason::parse(stop.as_deref()).is_complete();
    let value = json!({"content":content.finish(complete)?,"stop_reason":stop,"model":model});
    let parsed = parse_response(&value)?.with_usage(usage.finish());
    Ok(if text.is_empty() {
        parsed
    } else {
        parsed.with_text(text)
    })
}
