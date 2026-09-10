use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_MESSAGES: usize = 256;
pub const MAX_MESSAGE_BYTES: usize = 2048;
pub const DEFAULT_TIMEOUT_MS: u64 = 300_000;
pub const MAX_TIMEOUT_MS: u64 = 3_600_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageStatus {
    Notice,
    Pending,
    Answered {
        by: String,
        body: String,
        declined: bool,
    },
    Expired,
    Cancelled {
        reason: String,
    },
}

impl MessageStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Notice => "notice",
            Self::Pending => "pending",
            Self::Answered {
                declined: false, ..
            } => "answered",
            Self::Answered { declined: true, .. } => "declined",
            Self::Expired => "expired",
            Self::Cancelled { .. } => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborationMessage {
    pub id: String,
    pub sequence: u64,
    pub from: String,
    pub to: String,
    pub plan_version: u64,
    pub from_attempt: u32,
    pub to_attempt: u32,
    pub body: String,
    pub deadline_ms: Option<u64>,
    pub status: MessageStatus,
    pub(crate) delivered: bool,
    pub(crate) response_seen: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborationState {
    pub(crate) sequence: u64,
    pub(crate) messages: BTreeMap<String, CollaborationMessage>,
}

impl CollaborationState {
    pub fn messages(&self) -> impl Iterator<Item = &CollaborationMessage> {
        self.messages.values()
    }
    pub fn get(&self, id: &str) -> Option<&CollaborationMessage> {
        self.messages.get(id)
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}
