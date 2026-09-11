use crate::ids::{CommitKey, Sha};
use crate::paths::RelPath;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepoHead {
    pub sha: Sha,
    pub branch: Option<String>,
    pub dirty: bool,
    pub dirty_files: u32,
    pub observed_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Binary,
}

impl ChangeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Modified => "modified",
            ChangeKind::Deleted => "deleted",
            ChangeKind::Renamed => "renamed",
            ChangeKind::Binary => "binary",
        }
    }
    pub fn parse(s: &str) -> ChangeKind {
        match s {
            "added" => ChangeKind::Added,
            "deleted" => ChangeKind::Deleted,
            "renamed" => ChangeKind::Renamed,
            "binary" => ChangeKind::Binary,
            _ => ChangeKind::Modified,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangedFile {
    /// Project-root-relative.
    pub path: RelPath,
    /// Repo-relative, as git reported it.
    pub repo_path: String,
    pub kind: ChangeKind,
    pub added: Option<u32>,
    pub deleted: Option<u32>,
    pub rename_from: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Commit {
    pub key: CommitKey,
    pub parents: Vec<Sha>,
    pub author: String,
    pub author_email: String,
    pub authored_at: Timestamp,
    pub committed_at: Timestamp,
    pub subject: String,
    pub body: String,
    /// Tags / branch names decorating the commit (`%D`).
    pub refs: Vec<String>,
    pub files: Vec<ChangedFile>,
    pub trailers: BTreeMap<String, Vec<String>>,
}

impl Commit {
    pub fn short(&self) -> &str {
        self.key.sha.short()
    }
    pub fn tags(&self) -> Vec<String> {
        self.refs.iter().filter_map(|r| r.strip_prefix("tag: ").map(|s| s.to_string())).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitOrigin {
    Declared,
    Sidecar,
    InferredByTimestamp,
    InferredHead,
}

impl CommitOrigin {
    pub fn as_str(&self) -> &'static str {
        match self {
            CommitOrigin::Declared => "declared",
            CommitOrigin::Sidecar => "sidecar",
            CommitOrigin::InferredByTimestamp => "inferred_by_timestamp",
            CommitOrigin::InferredHead => "inferred_head",
        }
    }
    pub fn parse(s: &str) -> CommitOrigin {
        match s {
            "declared" => CommitOrigin::Declared,
            "sidecar" => CommitOrigin::Sidecar,
            "inferred_head" => CommitOrigin::InferredHead,
            _ => CommitOrigin::InferredByTimestamp,
        }
    }
}

/// A reference to a commit as evidence claims it; `resolved` says whether the local history
/// actually contains it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitRef {
    pub key: CommitKey,
    pub origin: CommitOrigin,
    pub resolved: bool,
}
