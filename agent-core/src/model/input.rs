//! Allocation-free logical input measurements; provider wire formatting is separate.
use super::{ModelError, ModelRequest};
use crate::context::{Message, Role};
use serde::{Serialize, ser::SerializeSeq};
use std::{
    hash::{DefaultHasher, Hasher},
    io::{self, Write},
};

#[derive(Debug, Serialize)]
pub struct InputDigest {
    pub bytes: usize,
    pub fingerprint: u64,
}
#[derive(Debug, Serialize)]
pub struct InputDiagnostics {
    pub system: InputDigest,
    pub history: InputDigest,
    pub memories: InputDigest,
    pub tools: InputDigest,
    pub logical_bytes: usize,
}
struct Messages<'a> {
    messages: &'a [Message],
    system: bool,
}
impl Serialize for Messages<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(None)?;
        for message in self
            .messages
            .iter()
            .filter(|m| (m.role() == Role::System) == self.system)
        {
            seq.serialize_element(message)?;
        }
        seq.end()
    }
}
struct DigestWriter {
    bytes: usize,
    hash: DefaultHasher,
}
impl Write for DigestWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("输入大小溢出"))?;
        self.hash.write(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn digest(value: &impl Serialize) -> Result<InputDigest, ModelError> {
    let mut writer = DigestWriter {
        bytes: 0,
        hash: DefaultHasher::new(),
    };
    serde_json::to_writer(&mut writer, value).map_err(ModelError::new)?;
    Ok(InputDigest {
        bytes: writer.bytes,
        fingerprint: writer.hash.finish(),
    })
}
impl ModelRequest<'_> {
    pub fn diagnostics(&self) -> Result<InputDiagnostics, ModelError> {
        let system = digest(&Messages {
            messages: self.messages(),
            system: true,
        })?;
        let history = digest(&Messages {
            messages: self.messages(),
            system: false,
        })?;
        let memories = digest(&self.memories())?;
        let tools = digest(&self.tools())?;
        let logical_bytes = system
            .bytes
            .saturating_add(history.bytes)
            .saturating_add(memories.bytes)
            .saturating_add(tools.bytes);
        Ok(InputDiagnostics {
            system,
            history,
            memories,
            tools,
            logical_bytes,
        })
    }
}

pub(crate) fn encoded_size(value: &impl Serialize) -> Result<usize, ModelError> {
    Ok(digest(value)?.bytes)
}
