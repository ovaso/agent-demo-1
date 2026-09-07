use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{Memory, MemoryStore, MemoryStoreError, validate_id};

const HEADER_PREFIX: &str = "<!-- rs-agent-memory: ";
const HEADER_SUFFIX: &str = " -->";

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

    fn read_memory(&self, path: &Path) -> Result<Memory, MemoryStoreError> {
        let document = fs::read_to_string(path).map_err(MemoryStoreError::storage)?;
        let (header, content) = document
            .split_once('\n')
            .ok_or_else(|| MemoryStoreError::InvalidMarkdown(path.display().to_string()))?;
        let json = header
            .strip_prefix(HEADER_PREFIX)
            .and_then(|value| value.strip_suffix(HEADER_SUFFIX))
            .ok_or_else(|| MemoryStoreError::InvalidMarkdown(path.display().to_string()))?;
        let mut memory: Memory =
            serde_json::from_str(json).map_err(MemoryStoreError::serialization)?;
        memory.content = content.to_owned();
        Ok(memory)
    }
}

impl MemoryStore for MarkdownMemoryStore {
    fn get(&self, id: &str) -> Result<Option<Memory>, MemoryStoreError> {
        validate_id(id)?;
        let path = self.path_for(id);
        if !path.exists() {
            return Ok(None);
        }
        self.read_memory(&path).map(Some)
    }

    fn save(&mut self, memory: Memory) -> Result<(), MemoryStoreError> {
        validate_id(memory.id())?;
        let path = self.path_for(memory.id());
        let header_memory = Memory {
            id: memory.id.clone(),
            content: String::new(),
            tags: memory.tags.clone(),
        };
        let header =
            serde_json::to_string(&header_memory).map_err(MemoryStoreError::serialization)?;
        let document = format!("{HEADER_PREFIX}{header}{HEADER_SUFFIX}\n{}", memory.content);
        let temporary = path.with_extension("md.tmp");
        fs::write(&temporary, document).map_err(MemoryStoreError::storage)?;
        fs::rename(temporary, path).map_err(MemoryStoreError::storage)?;
        Ok(())
    }

    fn list(&self) -> Result<Vec<Memory>, MemoryStoreError> {
        let mut memories = Vec::new();
        for entry in fs::read_dir(&self.directory).map_err(MemoryStoreError::storage)? {
            let path = entry.map_err(MemoryStoreError::storage)?.path();
            if path.extension().is_some_and(|extension| extension == "md") {
                memories.push(self.read_memory(&path)?);
            }
        }
        memories.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(memories)
    }

    fn search(&self, query: &str) -> Result<Vec<Memory>, MemoryStoreError> {
        let query = query.to_lowercase();
        Ok(self
            .list()?
            .into_iter()
            .filter(|memory| {
                memory.id.to_lowercase().contains(&query)
                    || memory.content.to_lowercase().contains(&query)
                    || memory
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&query))
            })
            .collect())
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

fn encode_id(id: &str) -> String {
    id.as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn saves_and_searches_markdown_memories() {
        let directory = std::env::temp_dir().join(format!(
            "rs-agent-memory-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = MarkdownMemoryStore::open(&directory).unwrap();
        let memory = Memory::new("rust", "Rust 的所有权模型避免悬垂引用")
            .with_tag("语言")
            .with_tag("所有权");
        store.save(memory.clone()).unwrap();
        assert_eq!(store.get("rust").unwrap(), Some(memory));
        assert_eq!(store.search("所有权").unwrap().len(), 1);
        assert!(store.delete("rust").unwrap());
        fs::remove_dir_all(directory).unwrap();
    }
}
