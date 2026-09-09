use super::RuntimeError;
use serde::{Deserialize, Serialize};

/// 根任务的硬上限；恢复使用已保存的配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunLimits {
    pub max_steps: u64,
    pub max_tool_calls: u64,
    pub max_transitions: u64,
    pub max_calls_per_response: usize,
    pub max_context_bytes: usize,
    pub max_tool_output_bytes: usize,
    pub max_checkpoint_bytes: usize,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            max_steps: 8,
            max_tool_calls: 256,
            max_transitions: 4096,
            max_calls_per_response: 32,
            max_context_bytes: 1024 * 1024,
            max_tool_output_bytes: 128 * 1024,
            max_checkpoint_bytes: 8 * 1024 * 1024,
        }
    }
}

impl RunLimits {
    pub fn new(max_steps: u64) -> Self {
        Self {
            max_steps,
            ..Self::default()
        }
    }

    pub(crate) fn validate(&self) -> Result<(), RuntimeError> {
        if self.max_steps == 0
            || self.max_tool_calls == 0
            || self.max_transitions == 0
            || self.max_calls_per_response == 0
            || self.max_context_bytes == 0
            || self.max_tool_output_bytes == 0
            || self.max_checkpoint_bytes == 0
        {
            return Err(RuntimeError::Invalid("预算和大小上限必须大于零".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunBudget {
    pub(crate) model_calls: u64,
    pub(crate) tool_calls: u64,
    pub(crate) transitions: u64,
}

impl RunBudget {
    pub fn model_calls(&self) -> u64 {
        self.model_calls
    }
    pub fn tool_calls(&self) -> u64 {
        self.tool_calls
    }
    pub fn transitions(&self) -> u64 {
        self.transitions
    }
}
