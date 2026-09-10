use super::runtime::RuntimeError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[default]
    Loop,
    Graph,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteChange {
    pub from: ExecutionMode,
    pub to: ExecutionMode,
    pub reason: String,
    pub plan_version: u64,
    pub model_calls_used: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingState {
    pub(crate) mode: ExecutionMode,
    pub(crate) pending: Option<(ExecutionMode, String)>,
    pub(crate) history: Vec<RouteChange>,
}

impl RoutingState {
    pub fn pending_mode(&self) -> Option<ExecutionMode> {
        self.pending.as_ref().map(|(mode, _)| *mode)
    }
    pub fn mode(&self) -> ExecutionMode {
        self.mode
    }
    pub fn history(&self) -> &[RouteChange] {
        &self.history
    }
    pub fn request(&mut self, mode: ExecutionMode, reason: &str) -> Result<(), RuntimeError> {
        if reason.trim().is_empty() || reason.len() > 2048 {
            return Err(RuntimeError::Invalid(
                "路由原因必须为非空且最多 2048 字节".into(),
            ));
        }
        if self.history.len() >= 16 && mode != self.mode {
            return Err(RuntimeError::Invalid("模式切换次数达到上限".into()));
        }
        self.pending = Some((mode, reason.into()));
        Ok(())
    }
}
