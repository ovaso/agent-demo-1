use agent_core::model::ModelError;
use agent_vendor::http::host_is;
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub(super) enum CacheTtl {
    Off,
    Short,
    Long,
}
impl CacheTtl {
    pub(super) fn parse(value: &str) -> Result<Self, ModelError> {
        match value {
            "off" | "0" => Ok(Self::Off),
            "5m" => Ok(Self::Short),
            "1h" => Ok(Self::Long),
            _ => Err(ModelError::new("缓存 TTL 必须为 off、5m 或 1h")),
        }
    }
}

pub(super) fn anthropic(body: &mut Value, configured: Option<CacheTtl>, url: &str) {
    let mode = configured.unwrap_or(if host_is(url, "api.anthropic.com") {
        CacheTtl::Short
    } else {
        CacheTtl::Off
    });
    let control = match mode {
        CacheTtl::Off => return,
        CacheTtl::Short => json!({"type":"ephemeral"}),
        CacheTtl::Long => json!({"type":"ephemeral","ttl":"1h"}),
    };
    body["cache_control"] = control.clone();
    let system = std::mem::take(&mut body["system"]);
    body["system"] = match system {
        Value::String(text) if !text.is_empty() => {
            json!([{"type":"text","text":text,"cache_control":control}])
        }
        other => other,
    };
    if let Some(last) = body["tools"]
        .as_array_mut()
        .and_then(|tools| tools.last_mut())
    {
        last["cache_control"] = control;
    }
}
