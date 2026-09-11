use crate::ids::{EventId, FeatureId};
use crate::model::provenance::EvidenceRef;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    FeatureChanged,
    FeatureValidated,
    FeatureBroken,
    FeatureRecovered,
    FeatureAttention,
    RegressionDetected,
    VisualRegressionDetected,
    EvidenceStale,
    AdrAccepted,
    AdrSuperseded,
    NewFeature,
    NewAdr,
    AttentionOpened,
    AttentionResolved,
    SuiteRan,
    ScanCompleted,
}

impl EventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventKind::FeatureChanged => "feature_changed",
            EventKind::FeatureValidated => "feature_validated",
            EventKind::FeatureBroken => "feature_broken",
            EventKind::FeatureRecovered => "feature_recovered",
            EventKind::FeatureAttention => "feature_attention",
            EventKind::RegressionDetected => "regression_detected",
            EventKind::VisualRegressionDetected => "visual_regression_detected",
            EventKind::EvidenceStale => "evidence_stale",
            EventKind::AdrAccepted => "adr_accepted",
            EventKind::AdrSuperseded => "adr_superseded",
            EventKind::NewFeature => "new_feature",
            EventKind::NewAdr => "new_adr",
            EventKind::AttentionOpened => "attention_opened",
            EventKind::AttentionResolved => "attention_resolved",
            EventKind::SuiteRan => "suite_ran",
            EventKind::ScanCompleted => "scan_completed",
        }
    }
    pub fn parse(s: &str) -> Option<EventKind> {
        use EventKind::*;
        Some(match s {
            "feature_changed" => FeatureChanged,
            "feature_validated" => FeatureValidated,
            "feature_broken" => FeatureBroken,
            "feature_recovered" => FeatureRecovered,
            "feature_attention" => FeatureAttention,
            "regression_detected" => RegressionDetected,
            "visual_regression_detected" => VisualRegressionDetected,
            "evidence_stale" => EvidenceStale,
            "adr_accepted" => AdrAccepted,
            "adr_superseded" => AdrSuperseded,
            "new_feature" => NewFeature,
            "new_adr" => NewAdr,
            "attention_opened" => AttentionOpened,
            "attention_resolved" => AttentionResolved,
            "suite_ran" => SuiteRan,
            "scan_completed" => ScanCompleted,
            _ => return None,
        })
    }
    pub fn label(&self) -> &'static str {
        match self {
            EventKind::FeatureChanged => "implementation changed",
            EventKind::FeatureValidated => "validated",
            EventKind::FeatureBroken => "became broken",
            EventKind::FeatureRecovered => "recovered",
            EventKind::FeatureAttention => "needs attention",
            EventKind::RegressionDetected => "performance regression detected",
            EventKind::VisualRegressionDetected => "visual regression detected",
            EventKind::EvidenceStale => "evidence became stale",
            EventKind::AdrAccepted => "ADR accepted",
            EventKind::AdrSuperseded => "ADR superseded",
            EventKind::NewFeature => "new feature",
            EventKind::NewAdr => "new ADR",
            EventKind::AttentionOpened => "attention opened",
            EventKind::AttentionResolved => "attention resolved",
            EventKind::SuiteRan => "suite ran",
            EventKind::ScanCompleted => "scan completed",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineeringEvent {
    pub id: EventId,
    pub kind: EventKind,
    pub at: Timestamp,
    pub feature: Option<FeatureId>,
    pub subject: Option<EvidenceRef>,
    pub text: String,
    pub payload: serde_json::Value,
}
