//! Bounded JSON traversal shared by validation and checkpoint encoding.
use super::RuntimeError;
use serde::Serialize;
use std::io::{self, Write};

pub(super) fn check(value: &impl Serialize, limit: usize) -> Result<(), RuntimeError> {
    write_json(value, None, limit)
}

#[cfg(feature = "sqlite")]
pub(super) fn encode(value: &impl Serialize, limit: usize) -> Result<String, RuntimeError> {
    let mut buffer = Vec::with_capacity(limit.min(128));
    write_json(value, Some(&mut buffer), limit)?;
    String::from_utf8(buffer).map_err(RuntimeError::storage)
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
    serde_json::to_writer(&mut writer, value)
        .map_err(|error| RuntimeError::Invalid(error.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_use_encoded_bytes_including_utf8_and_escapes() {
        let value = ["中文😀", "\"\\\n\u{0}"];
        let expected = serde_json::to_string(&value).unwrap();
        assert!(check(&value, expected.len()).is_ok());
        assert!(check(&value, expected.len() - 1).is_err());
        #[cfg(feature = "sqlite")]
        {
            assert_eq!(encode(&value, expected.len()).unwrap(), expected);
            assert!(encode(&value, expected.len() - 1).is_err());
        }
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn encoding_traverses_once_and_propagates_serializer_errors() {
        use std::cell::Cell;
        struct Once(Cell<usize>);
        impl Serialize for Once {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.0.set(self.0.get() + 1);
                if self.0.get() > 1 {
                    return Err(serde::ser::Error::custom("second traversal"));
                }
                serializer.serialize_str("once")
            }
        }
        let value = Once(Cell::new(0));
        assert_eq!(encode(&value, 6).unwrap(), "\"once\"");
        assert_eq!(value.0.get(), 1);
        assert!(encode(&value, 6).is_err());
    }
}
