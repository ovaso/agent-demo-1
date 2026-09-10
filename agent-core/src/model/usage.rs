use serde::{Deserialize, Serialize};

/// 服务端报告的单次请求用量。None 表示未报告，不能当作 0。
/// input_tokens 包含缓存读取/写入；reasoning_tokens 是 output_tokens 的子集。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_write_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

impl ModelUsage {
    pub fn total_tokens(&self) -> Option<u64> {
        self.input_tokens?.checked_add(self.output_tokens?)
    }

    pub fn cache_hit(&self) -> Option<bool> {
        self.cached_input_tokens.map(|tokens| tokens > 0)
    }

    /// 缓存读取占全部输入的比例；缺失、零分母或不一致的报告不伪造比例。
    pub fn cache_read_percent(&self) -> Option<f64> {
        let input = self.input_tokens?;
        let cached = self.cached_input_tokens?;
        (input > 0 && cached <= input).then(|| cached as f64 * 100.0 / input as f64)
    }

    pub(crate) fn zero() -> Self {
        Self {
            input_tokens: Some(0),
            output_tokens: Some(0),
            cached_input_tokens: Some(0),
            cache_write_input_tokens: Some(0),
            reasoning_tokens: Some(0),
        }
    }

    pub(crate) fn add(&mut self, other: Self) {
        fn sum(left: Option<u64>, right: Option<u64>) -> Option<u64> {
            left?.checked_add(right?)
        }
        self.input_tokens = sum(self.input_tokens, other.input_tokens);
        self.output_tokens = sum(self.output_tokens, other.output_tokens);
        self.cached_input_tokens = sum(self.cached_input_tokens, other.cached_input_tokens);
        self.cache_write_input_tokens = sum(
            self.cache_write_input_tokens,
            other.cache_write_input_tokens,
        );
        self.reasoning_tokens = sum(self.reasoning_tokens, other.reasoning_tokens);
    }
}
