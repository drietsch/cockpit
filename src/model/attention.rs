use crate::ids::{AttentionId, FeatureId};
use crate::model::provenance::EvidenceRef;
use crate::model::relationship::Confidence;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionReason {
    TestFailed,
    TestFlaky,
    TestDurationRegression,
    VisualRegression,
    VisualMissing,
    VisualStale,
    BenchmarkRegression,
    BenchmarkBudgetBreach,
    BenchmarkIncomparable,
    MissingRequiredTest,
    StaleEvidence,
    UnresolvedReference,
    IncompleteRevalidation,
    UnlinkedTests,
    UnverifiedClaim,
    EvidenceDrift,
    SuiteSilent,
    ImportError,
    EvidenceInvalid,
    PolicyFailure,
    NoEvidence,
}

impl AttentionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            AttentionReason::TestFailed => "test_failed",
            AttentionReason::TestFlaky => "test_flaky",
            AttentionReason::TestDurationRegression => "test_duration_regression",
            AttentionReason::VisualRegression => "visual_regression",
            AttentionReason::VisualMissing => "visual_missing",
            AttentionReason::VisualStale => "visual_stale",
            AttentionReason::BenchmarkRegression => "benchmark_regression",
            AttentionReason::BenchmarkBudgetBreach => "benchmark_budget_breach",
            AttentionReason::BenchmarkIncomparable => "benchmark_incomparable",
            AttentionReason::MissingRequiredTest => "missing_required_test",
            AttentionReason::StaleEvidence => "stale_evidence",
            AttentionReason::UnresolvedReference => "unresolved_reference",
            AttentionReason::IncompleteRevalidation => "incomplete_revalidation",
            AttentionReason::UnlinkedTests => "unlinked_tests",
            AttentionReason::UnverifiedClaim => "unverified_claim",
            AttentionReason::EvidenceDrift => "evidence_drift",
            AttentionReason::SuiteSilent => "suite_silent",
            AttentionReason::ImportError => "import_error",
            AttentionReason::EvidenceInvalid => "evidence_invalid",
            AttentionReason::PolicyFailure => "policy_failure",
            AttentionReason::NoEvidence => "no_evidence",
        }
    }
    pub fn parse(s: &str) -> Option<AttentionReason> {
        use AttentionReason::*;
        Some(match s {
            "test_failed" => TestFailed,
            "test_flaky" => TestFlaky,
            "test_duration_regression" => TestDurationRegression,
            "visual_regression" => VisualRegression,
            "visual_missing" => VisualMissing,
            "visual_stale" => VisualStale,
            "benchmark_regression" => BenchmarkRegression,
            "benchmark_budget_breach" => BenchmarkBudgetBreach,
            "benchmark_incomparable" => BenchmarkIncomparable,
            "missing_required_test" => MissingRequiredTest,
            "stale_evidence" => StaleEvidence,
            "unresolved_reference" => UnresolvedReference,
            "incomplete_revalidation" => IncompleteRevalidation,
            "unlinked_tests" => UnlinkedTests,
            "unverified_claim" => UnverifiedClaim,
            "evidence_drift" => EvidenceDrift,
            "suite_silent" => SuiteSilent,
            "import_error" => ImportError,
            "evidence_invalid" => EvidenceInvalid,
            "policy_failure" => PolicyFailure,
            "no_evidence" => NoEvidence,
            _ => return None,
        })
    }
    pub fn label(&self) -> &'static str {
        match self {
            AttentionReason::TestFailed => "Test failure",
            AttentionReason::TestFlaky => "Flaky test",
            AttentionReason::TestDurationRegression => "Test duration regression",
            AttentionReason::VisualRegression => "Visual regression",
            AttentionReason::VisualMissing => "Visual evidence missing",
            AttentionReason::VisualStale => "Visual evidence stale",
            AttentionReason::BenchmarkRegression => "Performance regression",
            AttentionReason::BenchmarkBudgetBreach => "Budget breach",
            AttentionReason::BenchmarkIncomparable => "Incomparable benchmark",
            AttentionReason::MissingRequiredTest => "Required evidence missing",
            AttentionReason::StaleEvidence => "Evidence stale",
            AttentionReason::UnresolvedReference => "Unresolved reference",
            AttentionReason::IncompleteRevalidation => "Incomplete revalidation",
            AttentionReason::UnlinkedTests => "Unlinked tests",
            AttentionReason::UnverifiedClaim => "Unverified claim",
            AttentionReason::EvidenceDrift => "Evidence drift",
            AttentionReason::SuiteSilent => "Suite silent",
            AttentionReason::ImportError => "Import error",
            AttentionReason::EvidenceInvalid => "Evidence invalid",
            AttentionReason::PolicyFailure => "Policy failure",
            AttentionReason::NoEvidence => "No evidence",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Informational,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Critical => "critical",
            Severity::High => "high",
            Severity::Medium => "medium",
            Severity::Low => "low",
            Severity::Informational => "informational",
        }
    }
    pub fn parse(s: &str) -> Severity {
        match s {
            "critical" => Severity::Critical,
            "high" => Severity::High,
            "medium" => Severity::Medium,
            "low" => Severity::Low,
            _ => Severity::Informational,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttentionStatus {
    Open,
    Resolved,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttentionItem {
    pub id: AttentionId,
    pub reason: AttentionReason,
    pub severity: Severity,
    pub status: AttentionStatus,
    pub feature: Option<FeatureId>,
    pub feature_title: Option<String>,
    pub subject: String,
    pub title: String,
    pub summary: String,
    pub why_it_matters: String,
    pub evidence: Vec<EvidenceRef>,
    pub confidence: Confidence,
    pub score: i32,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    pub resolved_at: Option<Timestamp>,
    pub commit: Option<crate::ids::CommitKey>,
    pub payload: serde_json::Value,
}
