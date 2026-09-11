use crate::ids::{ArtifactId, RunId, TestId, TestRunId, VisualCheckId, VisualEvidenceId};
use crate::model::git::CommitRef;
use crate::time::Stamped;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisualStatus {
    Current,
    Regression,
    /// No baseline to compare against.
    Missing,
    Stale,
    Inconclusive,
    Skipped,
}

impl VisualStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            VisualStatus::Current => "current",
            VisualStatus::Regression => "regression",
            VisualStatus::Missing => "missing",
            VisualStatus::Stale => "stale",
            VisualStatus::Inconclusive => "inconclusive",
            VisualStatus::Skipped => "skipped",
        }
    }
    pub fn parse(s: &str) -> VisualStatus {
        match s {
            "current" => VisualStatus::Current,
            "regression" => VisualStatus::Regression,
            "missing" => VisualStatus::Missing,
            "stale" => VisualStatus::Stale,
            "skipped" => VisualStatus::Skipped,
            _ => VisualStatus::Inconclusive,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DiffMetrics {
    pub changed_pixels: Option<u64>,
    pub difference_ratio: Option<f64>,
    pub mean_de: Option<f64>,
    pub p99_de: Option<f64>,
    pub max_de: Option<f64>,
    pub ssim: Option<f64>,
    pub thresholds: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VisualEvidence {
    pub id: VisualEvidenceId,
    pub check: VisualCheckId,
    pub run: RunId,
    pub name: String,
    pub test: Option<TestId>,
    pub test_run: Option<TestRunId>,
    pub page: Option<u32>,
    pub browser: Option<String>,
    pub viewport: Option<Viewport>,
    pub baseline: Option<ArtifactId>,
    pub actual: Option<ArtifactId>,
    pub diff: Option<ArtifactId>,
    pub metrics: DiffMetrics,
    pub status: VisualStatus,
    pub commit: Option<CommitRef>,
    pub timestamp: Stamped<Timestamp>,
    pub declared_feature_ids: Vec<String>,
}
