use agent_core::model::ModelError;
use serde_json::{Value, json};
use std::{fmt, str::FromStr};

/// DeepSeek 原生思考档位。Default 不发送开关或强度，沿用模型默认值。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReasoningEffort {
    #[default]
    Default,
    None,
    Low,
    High,
    Max,
}

impl FromStr for ReasoningEffort {
    type Err = ModelError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "default" => Ok(Self::Default),
            "none" => Ok(Self::None),
            "low" => Ok(Self::Low),
            "high" => Ok(Self::High),
            "max" => Ok(Self::Max),
            _ => Err(ModelError::new(
                "DeepSeek 推理强度必须为 default、none、low、high 或 max",
            )),
        }
    }
}

impl fmt::Display for ReasoningEffort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Default => "default",
            Self::None => "none",
            Self::Low => "low",
            Self::High => "high",
            Self::Max => "max",
        })
    }
}

impl ReasoningEffort {
    pub(super) fn apply(self, body: &mut Value) {
        match self {
            Self::Default => {}
            Self::None => body["thinking"] = json!({"type": "disabled"}),
            _ => {
                body["thinking"] = json!({"type": "enabled"});
                body["reasoning_effort"] = json!(self.to_string());
            }
        }
    }
}
