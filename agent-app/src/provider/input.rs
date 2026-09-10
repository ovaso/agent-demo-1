//! Enforce the complete JSON request bound once and reuse that buffer for HTTP.
use agent_core::model::ModelError;
use serde_json::Value;
use std::io::{self, Write};

pub(super) fn encode(value: &Value, limit: usize) -> Result<Vec<u8>, ModelError> {
    let mut writer = Body {
        buffer: Vec::with_capacity(limit.min(4096)),
        limit,
    };
    serde_json::to_writer(&mut writer, value).map_err(ModelError::new)?;
    Ok(writer.buffer)
}
struct Body {
    buffer: Vec<u8>,
    limit: usize,
}
impl Write for Body {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.buffer.len()) {
            return Err(io::Error::other(
                "完整模型请求超过字节上限（含工具、记忆和协议字段）",
            ));
        }
        self.buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
