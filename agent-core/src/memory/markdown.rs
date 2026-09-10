use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{Memory, MemoryStore, MemoryStoreError, validate_id};

mod format;

/// 每条记忆一个 Markdown 文件的长期记忆存储。
pub struct MarkdownMemoryStore {
    directory: PathBuf,
}

impl MarkdownMemoryStore {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, MemoryStoreError> {
        let directory = directory.as_ref().to_owned();
        fs::create_dir_all(&directory).map_err(MemoryStoreError::storage)?;
        Ok(Self { directory })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.directory.join(format!("{}.md", encode_id(id)))
    }

    fn read_matching(&self, query: Option<&str>) -> Result<Vec<Memory>, MemoryStoreError> {
        let mut memories = Vec::new();
        for entry in fs::read_dir(&self.directory).map_err(MemoryStoreError::storage)? {
            let path = entry.map_err(MemoryStoreError::storage)?.path();
            if path.extension().is_some_and(|extension| extension == "md") {
                let memory = format::read(&path)?;
                if query.is_none_or(|query| matches(&memory, query)) {
                    memories.push(memory);
                }
            }
        }
        memories.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(memories)
    }
}

impl MemoryStore for MarkdownMemoryStore {
    fn get(&self, id: &str) -> Result<Option<Memory>, MemoryStoreError> {
        validate_id(id)?;
        let path = self.path_for(id);
        if !path.exists() {
            return Ok(None);
        }
        format::read(&path).map(Some)
    }

    fn save(&mut self, memory: Memory) -> Result<(), MemoryStoreError> {
        validate_id(memory.id())?;
        format::write(&self.path_for(memory.id()), &memory)
    }

    fn list(&self) -> Result<Vec<Memory>, MemoryStoreError> {
        self.read_matching(None)
    }

    fn search(&self, query: &str) -> Result<Vec<Memory>, MemoryStoreError> {
        self.read_matching(Some(&query.to_lowercase()))
    }

    fn search_bounded(
        &self,
        query: &str,
        limits: super::MemorySearchLimits,
    ) -> Result<super::MemorySelection, MemoryStoreError> {
        if limits.max_results == 0 {
            return Ok(super::MemorySelection::default());
        }
        let directory = self
            .directory
            .canonicalize()
            .map_err(MemoryStoreError::storage)?;
        let mut selection = super::selection::Selector::new(limits);
        let query = query.to_lowercase();
        for entry in fs::read_dir(directory).map_err(MemoryStoreError::storage)? {
            let path = entry.map_err(MemoryStoreError::storage)?.path();
            if path.extension().is_none_or(|extension| extension != "md") {
                continue;
            }
            match format::read_bounded(&path, limits.max_entry_bytes)? {
                Some(memory) if matches(&memory, &query) => {
                    selection.add(memory.with_source(path.to_string_lossy().into_owned()))
                }
                Some(_) => {}
                None => selection.truncated = true,
            }
        }
        Ok(selection.finish())
    }

    fn delete(&mut self, id: &str) -> Result<bool, MemoryStoreError> {
        validate_id(id)?;
        let path = self.path_for(id);
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(path).map_err(MemoryStoreError::storage)?;
        Ok(true)
    }
}

fn matches(memory: &Memory, query: &str) -> bool {
    memory.id.to_lowercase().contains(query)
        || memory.content.to_lowercase().contains(query)
        || memory
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(query))
}

fn encode_id(id: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(id.len().saturating_mul(2));
    for byte in id.bytes() {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 15) as usize] as char);
    }
    encoded
}
