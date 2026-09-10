use super::RunLimits;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkIntent {
    #[default]
    Execute,
    PlanOnly,
}

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub limits: RunLimits,
    pub intent: WorkIntent,
    pub planning: bool,
}

impl RunOptions {
    pub fn plan_only(limits: RunLimits) -> Self {
        Self {
            limits,
            intent: WorkIntent::PlanOnly,
            planning: true,
        }
    }
}
