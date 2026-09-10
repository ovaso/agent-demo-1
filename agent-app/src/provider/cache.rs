use agent_core::model::ModelError;
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
            _ => Err(ModelError::new("ANTHROPIC_CACHE_TTL 必须为 off、5m 或 1h")),
        }
    }
}
pub(super) fn host_is(url: &str, host: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .is_some_and(|u| u.host_str() == Some(host))
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
pub(super) fn validate_effort(value: Option<&str>) -> Result<(), ModelError> {
    if value.is_some_and(|v| {
        !matches!(
            v,
            "default" | "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
        )
    }) {
        return Err(ModelError::new("OPENAI_REASONING_EFFORT 取值不支持"));
    }
    Ok(())
}
pub(super) fn reasoning(body: &mut Value, effort: Option<&str>, url: &str) {
    let Some(effort) = effort.filter(|v| *v != "default") else {
        return;
    };
    if host_is(url, "api.deepseek.com") {
        body["thinking"] = json!({"type":if effort=="none" {"disabled"} else {"enabled"}});
        if effort != "none" {
            body["reasoning_effort"] = json!(effort);
        }
    } else {
        body["reasoning_effort"] = json!(effort);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compatible_hosts_opt_in_and_use_at_most_three_breakpoints() {
        let mut body = json!({"system":"fixed","messages":[],"tools":[{"name":"read"}]});
        anthropic(&mut body, None, "https://compatible.example/v1");
        assert!(body.get("cache_control").is_none());
        anthropic(
            &mut body,
            Some(CacheTtl::Long),
            "https://compatible.example/v1",
        );
        assert_eq!(body["cache_control"]["ttl"], "1h");
        assert_eq!(body["system"][0]["cache_control"]["ttl"], "1h");
        assert_eq!(body["tools"][0]["cache_control"]["ttl"], "1h");
    }
    #[test]
    fn reasoning_is_explicit_and_deepseek_uses_its_own_switch() {
        let mut body = json!({});
        reasoning(&mut body, None, "https://api.deepseek.com");
        assert_eq!(body, json!({}));
        reasoning(&mut body, Some("low"), "https://api.deepseek.com");
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "low");
        let mut body = json!({});
        reasoning(&mut body, Some("none"), "https://api.deepseek.com");
        assert_eq!(body, json!({"thinking":{"type":"disabled"}}));
        assert!(validate_effort(Some("invalid")).is_err());
    }
}
