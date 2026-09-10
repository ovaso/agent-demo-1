//! On-disk Markdown format; readers retain compatibility with existing notes.
use super::{Memory, MemoryStoreError};
use serde::Serialize;
use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};

const HEADER_PREFIX: &str = "<!-- rs-agent-memory: ";
const HEADER_SUFFIX: &str = " -->";

pub(super) fn read(path: &Path) -> Result<Memory, MemoryStoreError> {
    let mut document = fs::read_to_string(path).map_err(MemoryStoreError::storage)?;
    let boundary = document
        .find('\n')
        .ok_or_else(|| MemoryStoreError::InvalidMarkdown(path.display().to_string()))?;
    let json = document[..boundary]
        .strip_prefix(HEADER_PREFIX)
        .and_then(|value| value.strip_suffix(HEADER_SUFFIX))
        .ok_or_else(|| MemoryStoreError::InvalidMarkdown(path.display().to_string()))?;
    let mut memory: Memory = serde_json::from_str(json).map_err(MemoryStoreError::serialization)?;
    // Reuse the file buffer for the body instead of copying every note's content.
    document.drain(..=boundary);
    if document.capacity() > document.len().saturating_mul(2) {
        document.shrink_to_fit();
    }
    memory.content = document;
    Ok(memory)
}

#[derive(Serialize)]
struct Header<'a> {
    id: &'a str,
    content: &'static str,
    tags: &'a [String],
}

pub(super) fn write(path: &Path, memory: &Memory) -> Result<(), MemoryStoreError> {
    let header = serde_json::to_string(&Header {
        id: memory.id(),
        content: "",
        tags: memory.tags(),
    })
    .map_err(MemoryStoreError::serialization)?;
    let temporary = path.with_extension("md.tmp");
    {
        let file = File::create(&temporary).map_err(MemoryStoreError::storage)?;
        let mut writer = BufWriter::new(file);
        for bytes in [
            HEADER_PREFIX.as_bytes(),
            header.as_bytes(),
            HEADER_SUFFIX.as_bytes(),
            b"\n",
            memory.content().as_bytes(),
        ] {
            writer.write_all(bytes).map_err(MemoryStoreError::storage)?;
        }
        writer.flush().map_err(MemoryStoreError::storage)?;
    }
    fs::rename(temporary, path).map_err(MemoryStoreError::storage)
}
