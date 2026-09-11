use crate::ids::ArtifactId;
use crate::paths::RelPath;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    Screenshot,
    Video,
    Trace,
    Log,
    Report,
    Image,
    Json,
    Other,
}

impl ArtifactKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArtifactKind::Screenshot => "screenshot",
            ArtifactKind::Video => "video",
            ArtifactKind::Trace => "trace",
            ArtifactKind::Log => "log",
            ArtifactKind::Report => "report",
            ArtifactKind::Image => "image",
            ArtifactKind::Json => "json",
            ArtifactKind::Other => "other",
        }
    }
    pub fn parse(s: &str) -> ArtifactKind {
        match s {
            "screenshot" => ArtifactKind::Screenshot,
            "video" => ArtifactKind::Video,
            "trace" => ArtifactKind::Trace,
            "log" => ArtifactKind::Log,
            "report" => ArtifactKind::Report,
            "image" => ArtifactKind::Image,
            "json" => ArtifactKind::Json,
            _ => ArtifactKind::Other,
        }
    }
    pub fn from_content_type(ct: &str) -> ArtifactKind {
        if ct.starts_with("image/") {
            ArtifactKind::Image
        } else if ct.starts_with("video/") {
            ArtifactKind::Video
        } else if ct == "application/zip" {
            ArtifactKind::Trace
        } else if ct == "application/json" {
            ArtifactKind::Json
        } else if ct.starts_with("text/") {
            ArtifactKind::Log
        } else {
            ArtifactKind::Other
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactRole {
    Expected,
    Actual,
    Diff,
    Heat,
    Plain,
}

impl ArtifactRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArtifactRole::Expected => "expected",
            ArtifactRole::Actual => "actual",
            ArtifactRole::Diff => "diff",
            ArtifactRole::Heat => "heat",
            ArtifactRole::Plain => "plain",
        }
    }
    pub fn parse(s: &str) -> ArtifactRole {
        match s {
            "expected" => ArtifactRole::Expected,
            "actual" => ArtifactRole::Actual,
            "diff" => ArtifactRole::Diff,
            "heat" => ArtifactRole::Heat,
            _ => ArtifactRole::Plain,
        }
    }
}

/// Where an artifact file lives. Serving is restricted to the project tree, the evidence cache
/// and explicitly allowed external roots.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "path", rename_all = "snake_case")]
pub enum ArtifactPath {
    Project(RelPath),
    EvidenceCache(RelPath),
    External(PathBuf),
    Url(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Artifact {
    pub id: ArtifactId,
    pub kind: ArtifactKind,
    pub role: ArtifactRole,
    pub name: String,
    pub path: ArtifactPath,
    pub content_type: Option<String>,
    pub size: Option<u64>,
    pub exists: bool,
}
