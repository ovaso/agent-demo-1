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

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-memory-query-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> MarkdownMemoryStore {
        MarkdownMemoryStore::open(&self.0).unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn streaming_search_preserves_sorting_unicode_and_all_match_fields() {
    let directory = Directory::new();
    let mut store = directory.store();
    for memory in [
        Memory::new("z-body", "İstanbul and NEEDLE"),
        Memory::new("n-needle-id", "other"),
        Memory::new("a-tag", "unrelated").with_tag("Needle"),
        Memory::new("skip", "large unrelated content ".repeat(4096)),
    ] {
        store.save(memory).unwrap();
    }
    let found = store.search("NEEDLE").unwrap();
    assert_eq!(
        found.iter().map(Memory::id).collect::<Vec<_>>(),
        ["a-tag", "n-needle-id", "z-body"]
    );
    assert_eq!(store.search("i\u{307}STANBUL").unwrap()[0].id(), "z-body");
    assert_eq!(store.search("").unwrap(), store.list().unwrap());
}

#[test]
fn unmatched_corrupt_notes_still_fail_and_temporary_files_are_ignored() {
    let directory = Directory::new();
    let store = directory.store();
    fs::write(directory.0.join("ignored.md.tmp"), "unfinished").unwrap();
    fs::write(directory.0.join("other.txt"), "unrelated").unwrap();
    assert!(store.search("absent").unwrap().is_empty());
    fs::write(directory.0.join("broken.md"), "not a memory\nbody").unwrap();
    assert!(matches!(
        store.search("absent"),
        Err(MemoryStoreError::InvalidMarkdown(_))
    ));
}

#[test]
fn existing_format_round_trips_exact_content_and_encoded_ids() {
    let directory = Directory::new();
    let mut store = directory.store();
    assert_eq!(encode_id("a/b"), "612f62");
    let id = "路径/😀";
    let memory = Memory::new(id, "\n\r\n正文\0\"\\\n").with_tag("quoted \"tag\"");
    store.save(memory.clone()).unwrap();
    let header = serde_json::json!({"id":id,"content":"ignored header body","tags":memory.tags()});
    fs::write(
        store.path_for(id),
        format!("<!-- rs-agent-memory: {header} -->\n{}", memory.content()),
    )
    .unwrap();
    assert_eq!(store.get(id).unwrap(), Some(memory));
    store.save(Memory::new(id, "")).unwrap();
    assert_eq!(store.get(id).unwrap().unwrap().content(), "");
    assert!(store.delete(id).unwrap());
    assert!(store.get(id).unwrap().is_none());
}

#[test]
fn bounded_memory_search_is_deterministic_and_does_not_load_large_files() {
    let directory = Directory::new();
    let mut store = directory.store();
    for id in ["z", "b", "a"] {
        store.save(Memory::new(id, "needle evidence")).unwrap();
    }
    store
        .save(Memory::new("large", "needle ".repeat(100_000)))
        .unwrap();
    let result = store
        .search_bounded(
            "needle",
            crate::memory::MemorySearchLimits {
                max_results: 2,
                max_total_bytes: 4096,
                max_entry_bytes: 1024,
            },
        )
        .unwrap();
    assert_eq!(
        result.memories.iter().map(Memory::id).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(result.truncated);
    assert!(
        result
            .memories
            .iter()
            .all(|m| Path::new(m.source().unwrap()).is_absolute())
    );
    let disabled = store
        .search_bounded(
            "needle",
            crate::memory::MemorySearchLimits {
                max_results: 0,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(disabled.memories.is_empty());
}
