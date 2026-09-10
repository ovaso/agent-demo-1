use agent_core::model::{ModelError, ModelRequest};
pub(super) const OPENAI: &str = "openai-chat-completions";
pub(super) const ANTHROPIC: &str = "anthropic-messages";
pub(super) const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

pub(super) fn binding(protocol: &str, model: &str, url: &str) -> String {
    // Do not persist URL credentials or query parameters. Ports may change on
    // local proxies without changing the model/host/protocol conversation.
    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    format!("{protocol}:{host}:{model}")
}

pub(super) fn validate(
    request: &ModelRequest<'_>,
    protocol: &str,
    binding: &str,
) -> Result<(), ModelError> {
    for continuation in request.messages().iter().filter_map(|m| m.continuation()) {
        if continuation.protocol() != protocol
            || continuation.binding().is_some_and(|saved| saved != binding)
        {
            return Err(ModelError::new(
                "模型续接数据与当前模型服务不一致，请使用新的会话",
            ));
        }
    }
    Ok(())
}
