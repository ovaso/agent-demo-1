use agent_core::model::{ModelError, ModelRequest};

pub fn binding(protocol: &str, model: &str, url: &str) -> String {
    // Do not persist URL credentials or query parameters. Ports may change on
    // local proxies without changing the model/host/protocol conversation.
    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    format!("{protocol}:{host}:{model}")
}

pub fn validate(
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
