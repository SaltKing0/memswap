use serde::{Deserialize, Serialize};

/// Canonical memory classification. Harness-native vocabularies (Claude's
/// user|feedback|project|reference, Codex task/task_outcome) map onto these and
/// are preserved per-entry in `source` / tags rather than driving the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Instruction,
    Fact,
    Project,
    User,
    Index,
    Other,
}

/// Whether an entry applies globally or is scoped to one project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Global,
    Project,
}

/// Provenance of a memory entry — where it came from, losslessly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub harness: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

/// A single memory entry. `content_hash` is blake3(body); the body itself lives
/// in the store's `objects/` directory, addressed by that hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub kind: EntryKind,
    pub title: String,
    pub body: String,
    pub scope: Scope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub source: Source,
    #[serde(default)]
    pub tags: Vec<String>,
    /// blake3(body), hex. Set by the store on write; verified by `verify`.
    pub content_hash: String,
}

impl Entry {
    pub fn compute_hash(&self) -> String {
        crate::hash::blake3_hex(self.body.as_bytes())
    }
}
