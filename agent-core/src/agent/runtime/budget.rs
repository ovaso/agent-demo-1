use super::RuntimeError;
use serde::{Deserialize, Serialize};

/// 根任务的资源限制；恢复使用已保存的配置及续期记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<crate::context::ContextWindow>,
    #[serde(default)]
    pub memory_limits: crate::memory::MemorySearchLimits,
    #[serde(default = "default_delegations")]
    pub max_delegations: usize,
    /// 当前已获准的累计模型步数；无续期策略时也是硬上限。
    pub max_steps: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_extension: Option<super::StepExtensionPolicy>,
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
            memory_limits: Default::default(),
            context_window: None,
            max_delegations: default_delegations(),
            max_steps: 8,
            step_extension: None,
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
            || self.max_delegations == 0
            || self.max_tool_calls == 0
            || self.max_transitions == 0
            || self.max_calls_per_response == 0
            || self.max_context_bytes == 0
            || self.max_tool_output_bytes == 0
            || self.max_checkpoint_bytes == 0
        {
            return Err(RuntimeError::Invalid("预算和大小上限必须大于零".into()));
        }
        if self.memory_limits.max_results > 0
            && (self.memory_limits.max_entry_bytes == 0 || self.memory_limits.max_total_bytes == 0)
        {
            return Err(RuntimeError::Invalid("启用记忆时字节上限必须大于零".into()));
        }
        if let Some(window) = self.context_window {
            window
                .validate(self.max_context_bytes)
                .map_err(|e| RuntimeError::Invalid(e.to_string()))?;
        }
        if let Some(policy) = &self.step_extension {
            policy.validate(self.max_steps)?;
        }
        Ok(())
    }
}

fn default_delegations() -> usize {
    8
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunBudget {
    pub(crate) model_calls: u64,
    pub(crate) tool_calls: u64,
    pub(crate) transitions: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) step_progress: Option<super::step_budget::StepProgress>,
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
    pub fn step_extensions(&self) -> &[super::StepExtension] {
        self.step_progress
            .as_ref()
            .map_or(&[], |progress| progress.extensions.as_slice())
    }
}
