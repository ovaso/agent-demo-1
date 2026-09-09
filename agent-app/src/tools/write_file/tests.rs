use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use agent_core::tool::Registry;

use super::*;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "rs-agent-write-file-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed),
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("创建测试目录失败：{error}"),
            }
        }
    }

    fn arguments(&self, path: &str, content: &str) -> Arguments {
        Arguments::new()
            .with("path", self.0.join(path).to_str().unwrap())
            .with("content", content)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.register(WriteFile::new()).unwrap();
    registry
}

#[test]
fn creates_parent_directories_and_preserves_conversation_text() {
    let directory = TestDirectory::new();
    let content = "用户：请保存\nAgent：\"你好\"\n```json\n{\"ok\":true}\n```\n";
    let arguments = directory.arguments("notes/conversation.md", content);
    let result = registry().invoke("write_file", &arguments).unwrap();

    assert_eq!(
        fs::read_to_string(directory.0.join("notes/conversation.md")).unwrap(),
        content
    );
    let report: serde_json::Value = serde_json::from_str(result.content()).unwrap();
    assert_eq!(report["bytes_written"], content.len());
    assert_eq!(report["path"], arguments.get("path").unwrap());
    assert!(!result.finishes_session());
}

#[test]
fn preserves_json_literals_and_empty_text() {
    let directory = TestDirectory::new();
    let registry = registry();
    for content in ["null", "123", "true", "\"quoted\"", "{\"ok\":true}", ""] {
        let arguments = directory
            .arguments("literal.txt", content)
            .with("mode", "overwrite");
        registry.invoke("write_file", &arguments).unwrap();
        assert_eq!(
            fs::read_to_string(directory.0.join("literal.txt")).unwrap(),
            content
        );
    }
}

#[test]
fn create_does_not_overwrite_existing_file() {
    let directory = TestDirectory::new();
    let registry = registry();
    registry
        .invoke("write_file", &directory.arguments("saved.txt", "original"))
        .unwrap();
    for mode in [None, Some("create")] {
        let mut arguments = directory.arguments("saved.txt", "replacement");
        if let Some(mode) = mode {
            arguments = arguments.with("mode", mode);
        }
        assert!(registry.invoke("write_file", &arguments).is_err());
        assert_eq!(
            fs::read_to_string(directory.0.join("saved.txt")).unwrap(),
            "original"
        );
    }
}

#[test]
fn overwrite_truncates_and_append_keeps_existing_text() {
    let directory = TestDirectory::new();
    let registry = registry();
    for (mode, content, expected) in [
        ("append", "long original", "long original"),
        ("overwrite", "短", "短"),
        ("append", "\n追加", "短\n追加"),
    ] {
        registry
            .invoke(
                "write_file",
                &directory.arguments("saved.txt", content).with("mode", mode),
            )
            .unwrap();
        assert_eq!(
            fs::read_to_string(directory.0.join("saved.txt")).unwrap(),
            expected
        );
    }
}

#[test]
fn rejects_invalid_arguments_before_creating_directories() {
    let directory = TestDirectory::new();
    let registry = registry();
    let arguments = directory
        .arguments("missing/saved.txt", "text")
        .with("mode", "invalid");
    assert!(registry.invoke("write_file", &arguments).is_err());
    assert!(!directory.0.join("missing").exists());
    for arguments in [
        Arguments::new().with("path", "").with("content", "text"),
        Arguments::new().with("content", "text"),
        Arguments::new().with("path", directory.0.join("missing.txt").to_str().unwrap()),
    ] {
        assert!(registry.invoke("write_file", &arguments).is_err());
    }
    assert!(!directory.0.join("missing.txt").exists());
}

#[test]
fn reports_filesystem_errors_with_target_path() {
    let directory = TestDirectory::new();
    let registry = registry();
    fs::write(directory.0.join("parent"), "file instead of directory").unwrap();
    for path in ["parent/child.txt", "."] {
        let arguments = directory.arguments(path, "text").with("mode", "overwrite");
        let error = registry
            .invoke("write_file", &arguments)
            .unwrap_err()
            .to_string();
        assert!(error.contains("写入文件"));
        assert!(error.contains(arguments.get("path").unwrap()));
    }
}
