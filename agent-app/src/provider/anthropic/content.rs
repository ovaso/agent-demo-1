//! Preserve content order and signed thinking blocks while consuming SSE incrementally.
use agent_core::model::ModelError;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Content {
    blocks: BTreeMap<usize, Value>,
    arguments: BTreeMap<usize, String>,
}
impl Content {
    pub(super) fn start(&mut self, index: usize, block: &Value) -> Result<(), ModelError> {
        self.check(index)?;
        self.blocks.insert(index, block.clone());
        Ok(())
    }
    pub(super) fn delta(&mut self, index: usize, delta: &Value) -> Result<(), ModelError> {
        self.check(index)?;
        let (kind, field, value) = match delta["type"].as_str() {
            Some("text_delta") => ("text", "text", delta["text"].as_str()),
            Some("thinking_delta") => ("thinking", "thinking", delta["thinking"].as_str()),
            Some("signature_delta") => ("thinking", "signature", delta["signature"].as_str()),
            Some("input_json_delta") => {
                if let Some(part) = delta["partial_json"].as_str() {
                    self.arguments.entry(index).or_default().push_str(part);
                }
                return Ok(());
            }
            _ => return Ok(()),
        };
        if let Some(part) = value {
            let block = self
                .blocks
                .entry(index)
                .or_insert_with(|| json!({"type":kind}));
            if block[field].is_null() {
                block[field] = Value::String(String::new());
            }
            let Value::String(text) = &mut block[field] else {
                return Err(ModelError::new("Anthropic 内容增量类型不匹配"));
            };
            text.push_str(part);
        }
        Ok(())
    }
    fn check(&self, index: usize) -> Result<(), ModelError> {
        if index >= 1024 {
            Err(ModelError::new("Anthropic 内容块数量超限"))
        } else {
            Ok(())
        }
    }
    pub(super) fn finish(mut self, complete: bool) -> Result<Vec<Value>, ModelError> {
        if complete {
            for (index, arguments) in self.arguments {
                self.blocks
                    .get_mut(&index)
                    .ok_or_else(|| ModelError::new("Anthropic 工具内容缺少开始事件"))?["input"] =
                    serde_json::from_str(&arguments).map_err(ModelError::new)?;
            }
        } else {
            self.blocks.retain(|_, block| block["type"] != "tool_use");
        }
        Ok(self.blocks.into_values().collect())
    }
}
