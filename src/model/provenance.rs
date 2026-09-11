use crate::config::EvidenceFormat;
use crate::hash::ContentHash;
use crate::ids::{EnvironmentId, SourceId};
use crate::model::git::CommitRef;
use crate::paths::RelPath;
use crate::time::Stamped;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Provenance {
    pub source: SourceId,
    pub source_path: RelPath,
    pub format: EvidenceFormat,
    pub importer_version: u32,
    pub content_hash: ContentHash,
    pub commit: Option<CommitRef>,
    pub branch: Option<String>,
    pub timestamp: Option<Stamped<Timestamp>>,
    pub tool: Option<ToolInfo>,
    pub environment: Option<EnvironmentId>,
    pub run_id: Option<String>,
    pub run_url: Option<String>,
    pub dirty: Option<bool>,
    pub imported_at: Timestamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Feature,
    Adr,
    Test,
    TestRun,
    Run,
    Suite,
    Benchmark,
    BenchmarkRun,
    Visual,
    Artifact,
    Commit,
    Attention,
}

/// A pointer to a piece of evidence, with a human label; the server turns it into a link.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub kind: EvidenceKind,
    pub id: String,
    pub label: String,
}

impl EvidenceRef {
    pub fn new(kind: EvidenceKind, id: impl Into<String>, label: impl Into<String>) -> Self {
        EvidenceRef { kind, id: id.into(), label: label.into() }
    }
}

/// A non-fatal problem found while importing a file. Fatal warnings mean nothing was imported
/// from that file; they surface as `import_error` attention items.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportWarning {
    pub path: RelPath,
    pub importer: String,
    pub message: String,
    pub location: Option<String>,
    pub fatal: bool,
}
