use agent_core::model::ModelUsage;
use serde_json::Value;

#[derive(Default)]
pub(super) struct Usage(ModelUsage);

impl Usage {
    pub(super) fn update(&mut self, response: &Value) {
        if response.get("usage").is_some_and(Value::is_object) {
            self.0 = parse(response);
        }
    }

    pub(super) fn finish(self) -> ModelUsage {
        self.0
    }
}

pub(super) fn parse(response: &Value) -> ModelUsage {
    let usage = &response["usage"];
    ModelUsage {
        input_tokens: usage["prompt_tokens"].as_u64(),
        output_tokens: usage["completion_tokens"].as_u64(),
        cached_input_tokens: usage
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        cache_write_input_tokens: None,
        reasoning_tokens: usage
            .pointer("/completion_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
    }
}
