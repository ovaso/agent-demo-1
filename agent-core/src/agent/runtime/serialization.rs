//! Bounded JSON traversal shared by validation and checkpoint encoding.
use super::RuntimeError;
use serde::Serialize;
use std::io::{self, Write};

pub(super) fn check(value: &impl Serialize, limit: usize) -> Result<(), RuntimeError> {
    write_json(value, None, limit)
}

#[cfg(feature = "sqlite")]
pub(super) fn encode(value: &impl Serialize, limit: usize) -> Result<String, RuntimeError> {
    encode_prefixed(value, "", limit)
}

pub(super) fn encode_prefixed(
    value: &impl Serialize,
    prefix: &str,
    limit: usize,
) -> Result<String, RuntimeError> {
    let remaining = limit
        .checked_sub(prefix.len())
        .ok_or_else(|| RuntimeError::Invalid("序列化字节数超限".into()))?;
    let mut buffer = Vec::with_capacity(limit.min(128).max(prefix.len()));
    buffer.extend_from_slice(prefix.as_bytes());
    write_json(value, Some(&mut buffer), remaining)?;
    String::from_utf8(buffer)
        .map_err(|error| RuntimeError::Invalid(format!("JSON 编码产生无效 UTF-8：{error}")))
}

fn write_json(
    value: &impl Serialize,
    buffer: Option<&mut Vec<u8>>,
    limit: usize,
) -> Result<(), RuntimeError> {
    let mut writer = BoundedWriter {
        buffer,
        remaining: limit,
    };
    serde_json::to_writer(&mut writer, value).map_err(RuntimeError::from)
}

struct BoundedWriter<'a> {
    buffer: Option<&'a mut Vec<u8>>,
    remaining: usize,
}

impl Write for BoundedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::other("序列化字节数超限"));
        }
        if let Some(buffer) = &mut self.buffer {
            buffer.extend_from_slice(bytes);
        }
        self.remaining -= bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
