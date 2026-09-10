use serde::{Deserialize, Serialize};

/// 协调者提出的单个工作委托；工具范围由运行时与父任务取交集并校验。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSpec {
    pub name: String,
    pub instruction: String,
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default = "default_steps")]
    pub max_steps: u64,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

fn default_steps() -> u64 {
    4
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPolicy {
    pub tools: Vec<String>,
    pub max_steps: u64,
    pub model_calls: u64,
    pub budget_changes: Vec<(u64, u64)>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeOrigin {
    #[default]
    Planned,
    Delegated,
}
