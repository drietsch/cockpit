use crate::ids::CommitKey;
use crate::model::attention::{AttentionReason, Severity};
use crate::model::provenance::EvidenceRef;
use crate::model::relationship::Confidence;
use crate::model::test::{EvidenceCategory, RunCounts};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthState {
    Broken,
    Attention,
    Unknown,
    Healthy,
}

impl HealthState {
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthState::Healthy => "healthy",
            HealthState::Attention => "attention",
            HealthState::Broken => "broken",
            HealthState::Unknown => "unknown",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            HealthState::Healthy => "Healthy",
            HealthState::Attention => "Needs Attention",
            HealthState::Broken => "Broken",
            HealthState::Unknown => "Unknown",
        }
    }
    pub fn parse(s: &str) -> Option<HealthState> {
        Some(match s {
            "healthy" => HealthState::Healthy,
            "attention" => HealthState::Attention,
            "broken" => HealthState::Broken,
            "unknown" => HealthState::Unknown,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeFreshness {
    /// Generated after the latest relevant implementation change.
    Fresh,
    /// Generated before it.
    Stale,
    /// Could not be determined (no commit, no implementation commit known).
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Freshness {
    pub change: ChangeFreshness,
    /// The implementation commit the evidence was compared against.
    pub against: Option<CommitKey>,
    pub age: Option<SignedDuration>,
    pub time_stale: bool,
    pub confidence: Confidence,
    pub note: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollupStatus {
    Pass,
    Fail,
    Flaky,
    Stale,
    /// A failure that predates the latest implementation change.
    StaleFail,
    Missing,
    Silent,
    Regression,
    BudgetBreach,
    Incomparable,
    Improved,
    Unknown,
}

impl RollupStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RollupStatus::Pass => "pass",
            RollupStatus::Fail => "fail",
            RollupStatus::Flaky => "flaky",
            RollupStatus::Stale => "stale",
            RollupStatus::StaleFail => "stale_fail",
            RollupStatus::Missing => "missing",
            RollupStatus::Silent => "silent",
            RollupStatus::Regression => "regression",
            RollupStatus::BudgetBreach => "budget_breach",
            RollupStatus::Incomparable => "incomparable",
            RollupStatus::Improved => "improved",
            RollupStatus::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KindRollup {
    pub category: EvidenceCategory,
    pub status: RollupStatus,
    pub counts: RunCounts,
    pub freshness: Option<Freshness>,
    pub latest: Option<Timestamp>,
    pub refs: Vec<EvidenceRef>,
    pub required: bool,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthReason {
    pub reason: AttentionReason,
    pub severity: Severity,
    pub text: String,
    pub evidence: Vec<EvidenceRef>,
    pub confidence: Confidence,
    pub subject: String,
    pub commit: Option<CommitKey>,
    pub category: Option<EvidenceCategory>,
}

/// One step of the "Why?" chain (§192): `because` / `and` / `but` / `therefore`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExplainStep {
    pub connective: String,
    pub text: String,
    pub refs: Vec<EvidenceRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeatureHealth {
    pub state: HealthState,
    pub confidence: Confidence,
    /// One sentence for the header.
    pub headline: String,
    pub reasons: Vec<HealthReason>,
    pub rollups: Vec<KindRollup>,
    pub chain: Vec<ExplainStep>,
    pub impl_commit: Option<CommitKey>,
    pub impl_committed_at: Option<Timestamp>,
    pub evaluated_at: Timestamp,
    /// Internal ranking score (never displayed).
    pub rank_score: i32,
}
