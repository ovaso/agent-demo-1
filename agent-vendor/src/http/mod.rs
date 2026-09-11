//! HTTP 发送与响应边界；鉴权和协议内容由各 Provider 负责。
mod input;

use agent_core::model::ModelError;
use reqwest::blocking::{RequestBuilder, Response};
use serde_json::Value;
use std::io::Read;

pub const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

pub struct HttpResponse {
    pub response: Response,
    pub request_bytes: usize,
}

pub fn send(
    request: RequestBuilder,
    body: &Value,
    max_input_bytes: usize,
) -> Result<HttpResponse, ModelError> {
    let body = input::encode(body, max_input_bytes)?;
    let request_bytes = body.len();
    let response = request
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .map_err(ModelError::new)?
        .error_for_status()
        .map_err(ModelError::new)?;
    Ok(HttpResponse {
        response,
        request_bytes,
    })
}

pub fn read_json(response: Response) -> Result<Value, ModelError> {
    serde_json::from_reader(response.take(MAX_RESPONSE_BYTES)).map_err(ModelError::new)
}

/// 按解析后的主机名判断协议策略，避免把 URL 前缀当作可信主机。
pub fn host_is(url: &str, host: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .is_some_and(|url| url.host_str() == Some(host))
}
