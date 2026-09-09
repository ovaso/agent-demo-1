use agent_core::model::ModelUsage;
use serde_json::Value;

#[derive(Default)]
pub(super) struct Usage {
    input: Option<u64>,
    output: Option<u64>,
    read: Option<u64>,
    write: Option<u64>,
}

impl Usage {
    pub(super) fn update(&mut self, response: &Value) {
        let usage = &response["usage"];
        // message_delta 的计数是累计值，只覆盖报告字段，不能累加分片。
        for (target, key) in [
            (&mut self.input, "input_tokens"),
            (&mut self.output, "output_tokens"),
            (&mut self.read, "cache_read_input_tokens"),
            (&mut self.write, "cache_creation_input_tokens"),
        ] {
            if let Some(value) = usage[key].as_u64() {
                *target = Some(value);
            }
        }
    }

    pub(super) fn finish(self) -> ModelUsage {
        ModelUsage {
            // Anthropic 的 input_tokens 不含缓存；字段缺失时总输入量未知。
            input_tokens: self
                .input
                .and_then(|input| input.checked_add(self.read?)?.checked_add(self.write?)),
            output_tokens: self.output,
            cached_input_tokens: self.read,
            cache_write_input_tokens: self.write,
            reasoning_tokens: None,
        }
    }
}

pub(super) fn parse(response: &Value) -> ModelUsage {
    let mut usage = Usage::default();
    usage.update(response);
    usage.finish()
}
