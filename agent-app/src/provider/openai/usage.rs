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
            .and_then(Value::as_u64)
            .or_else(|| usage["prompt_cache_hit_tokens"].as_u64()),
        cache_write_input_tokens: usage
            .pointer("/prompt_tokens_details/cache_write_tokens")
            .and_then(Value::as_u64),
        reasoning_tokens: usage
            .pointer("/completion_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_deepseek_cache_tokens_without_double_counting() {
        let usage = parse(
            &json!({"usage":{"prompt_tokens":5000,"completion_tokens":100,
            "prompt_cache_hit_tokens":4000,"prompt_cache_miss_tokens":1000}}),
        );
        assert_eq!(usage.cached_input_tokens, Some(4000));
        assert_eq!(usage.total_tokens(), Some(5100));
    }

    #[test]
    fn explicit_zero_wins_and_absent_or_invalid_usage_stays_unknown() {
        let usage = parse(
            &json!({"usage":{"prompt_tokens_details":{"cached_tokens":0},
            "prompt_cache_hit_tokens":20}}),
        );
        assert_eq!(usage.cached_input_tokens, Some(0));
        assert_eq!(parse(&json!({})).cached_input_tokens, None);
        assert_eq!(
            parse(&json!({"usage":{"prompt_cache_hit_tokens":-1}})).cached_input_tokens,
            None
        );
    }
}
