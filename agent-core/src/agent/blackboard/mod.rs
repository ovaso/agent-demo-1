//! 任务内有来源、修订与读取上限的共享记录。
use super::runtime::RuntimeError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_BOARD_REVISIONS: usize = 256;
pub const MAX_ENTRY_BYTES: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Observation,
    Hypothesis,
    Decision,
    Blocker,
    Artifact,
    Verification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    pub uri: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardUpdate {
    pub key: String,
    pub expected_revision: u64,
    pub kind: EntryKind,
    pub content: String,
    #[serde(default)]
    pub sources: Vec<SourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardEntry {
    pub key: String,
    pub revision: u64,
    pub sequence: u64,
    pub author: String,
    pub plan_version: u64,
    pub kind: EntryKind,
    pub content: String,
    pub sources: Vec<SourceRef>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blackboard {
    entries: BTreeMap<String, Vec<BoardEntry>>,
    sequence: u64,
}

impl Blackboard {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn latest(&self, key: &str) -> Option<&BoardEntry> {
        self.entries.get(key)?.last()
    }
    pub fn version(&self, key: &str, revision: u64) -> Option<&BoardEntry> {
        revision
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| self.entries.get(key)?.get(index))
    }
    /// 返回发生变化的最新条目，消费位置由调用方检查点保存。
    pub fn changes(&self, after: u64, limit: usize) -> Vec<&BoardEntry> {
        let mut entries: Vec<_> = self
            .entries
            .values()
            .filter_map(|versions| versions.last())
            .filter(|entry| entry.sequence > after)
            .collect();
        entries.sort_by_key(|entry| entry.sequence);
        entries.truncate(limit.min(32));
        entries
    }
    pub fn write(
        &mut self,
        author: &str,
        plan_version: u64,
        update: BoardUpdate,
    ) -> Result<&BoardEntry, RuntimeError> {
        if update.key.is_empty()
            || update.key.len() > 128
            || update.key.contains(char::is_whitespace)
            || author.is_empty()
            || author.len() > 256
            || update.content.len() > MAX_ENTRY_BYTES
            || update.sources.len() > 16
            || update.sources.iter().any(|source| {
                source.uri.is_empty()
                    || source.uri.len() > 2048
                    || source.version.is_empty()
                    || source.version.len() > 256
            })
        {
            return Err(RuntimeError::Invalid(
                "共享记录的标识、大小或来源版本无效".into(),
            ));
        }
        if self.sequence as usize >= MAX_BOARD_REVISIONS {
            return Err(RuntimeError::Invalid("共享记录修订总数达到上限".into()));
        }
        if serde_json::to_vec(&update)
            .map_err(RuntimeError::storage)?
            .len()
            > MAX_ENTRY_BYTES
        {
            return Err(RuntimeError::Invalid(
                "共享记录及来源总大小超过 8 KiB".into(),
            ));
        }
        let revision = self.latest(&update.key).map_or(0, |entry| entry.revision);
        if revision != update.expected_revision {
            return Err(RuntimeError::Conflict);
        }
        self.sequence += 1;
        let entries = self.entries.entry(update.key.clone()).or_default();
        entries.push(BoardEntry {
            key: update.key,
            revision: revision + 1,
            sequence: self.sequence,
            author: author.into(),
            plan_version,
            kind: update.kind,
            content: update.content,
            sources: update.sources,
        });
        Ok(entries.last().expect("inserted entry"))
    }
}
