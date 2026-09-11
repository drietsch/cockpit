//! Feature health evaluation (§21–§22). M0 knows about implementation commits and unresolved
//! references; evidence rollups arrive with the test/benchmark/visual importers.

use crate::config::Config;
use crate::ids::{CommitKey, FeatureId};
use crate::model::*;
use jiff::Timestamp;

/// Evidence gathered for one feature (empty until evidence importers exist).
#[derive(Default, Debug, Clone)]
pub struct Evidence {
    pub rollups: Vec<KindRollup>,
}

pub fn evaluate(
    feature: &Feature,
    evidence: &Evidence,
    impl_commit: Option<&CommitKey>,
    impl_at: Option<Timestamp>,
    unresolved: &[UnresolvedRef],
    cfg: &Config,
    now: Timestamp,
) -> FeatureHealth {
    let mut reasons: Vec<HealthReason> = Vec::new();
    let mut chain: Vec<ExplainStep> = Vec::new();
    let importance = feature.importance.as_str();
    let required = cfg.health.required_for(importance);
    let has_any = evidence
        .rollups
        .iter()
        .any(|r| !matches!(r.status, RollupStatus::Missing | RollupStatus::Silent));

    for u in
        unresolved.iter().filter(|u| matches!(&u.from, EntityRef::Feature(f) if f == &feature.id))
    {
        reasons.push(HealthReason {
            reason: AttentionReason::UnresolvedReference,
            severity: Severity::Low,
            text: u.note.clone().unwrap_or_else(|| format!("{} does not resolve", u.raw)),
            evidence: vec![],
            confidence: Confidence::High,
            subject: u.raw.clone(),
            commit: None,
            category: None,
        });
    }

    let state;
    let confidence;
    if !has_any {
        state = HealthState::Unknown;
        confidence = Confidence::High;
        chain.push(ExplainStep {
            connective: "given".into(),
            text: "no test, visual or performance evidence is linked to this feature".into(),
            refs: vec![],
        });
        if !matches!(feature.lifecycle, Lifecycle::Planned | Lifecycle::Retired)
            && !required.is_empty()
        {
            let sev = match feature.importance {
                Importance::Critical => Severity::High,
                Importance::High => Severity::Medium,
                _ => Severity::Low,
            };
            reasons.push(HealthReason {
                reason: AttentionReason::MissingRequiredTest,
                severity: sev,
                text: format!(
                    "{} feature requires {} evidence; none is linked",
                    importance,
                    required.join(" + ")
                ),
                evidence: vec![],
                confidence: Confidence::High,
                subject: required.join("+"),
                commit: None,
                category: None,
            });
            chain.push(ExplainStep {
                connective: "and".into(),
                text: format!(
                    "a {importance} feature requires {} evidence",
                    required.join(" and ")
                ),
                refs: vec![],
            });
        }
        chain.push(ExplainStep {
            connective: "therefore".into(),
            text: "Unknown — missing evidence is never interpreted as healthy".into(),
            refs: vec![],
        });
    } else {
        // Evidence-driven states are computed in the M1 evaluator.
        state = HealthState::Healthy;
        confidence = Confidence::Medium;
    }

    let headline = match state {
        HealthState::Unknown => match impl_at {
            Some(t) => format!(
                "No evidence linked yet; implementation last changed {}",
                crate::time::ago(t, now)
            ),
            None => "No evidence linked yet".into(),
        },
        HealthState::Healthy => "All linked evidence is passing".into(),
        HealthState::Attention => {
            reasons.first().map(|r| r.text.clone()).unwrap_or_else(|| "Needs attention".into())
        }
        HealthState::Broken => {
            reasons.first().map(|r| r.text.clone()).unwrap_or_else(|| "Broken".into())
        }
    };

    let rank_score = rank(feature, state, &reasons, now, impl_at);
    FeatureHealth {
        state,
        confidence,
        headline,
        reasons,
        rollups: evidence.rollups.clone(),
        chain,
        impl_commit: impl_commit.cloned(),
        impl_committed_at: impl_at,
        evaluated_at: now,
        rank_score,
    }
}

/// Importance-adjusted attention score for ordering (never displayed).
pub fn rank(
    feature: &Feature,
    state: HealthState,
    reasons: &[HealthReason],
    now: Timestamp,
    changed_at: Option<Timestamp>,
) -> i32 {
    let mut s = match feature.importance {
        Importance::Critical => 50,
        Importance::High => 30,
        Importance::Normal => 15,
        Importance::Low => 5,
    };
    s += match state {
        HealthState::Broken => 400,
        HealthState::Attention => 200,
        HealthState::Unknown => 100,
        HealthState::Healthy => 0,
    };
    for r in reasons {
        s += match r.severity {
            Severity::Critical => 40,
            Severity::High => 30,
            Severity::Medium => 20,
            Severity::Low => 10,
            Severity::Informational => 0,
        };
    }
    if reasons.len() >= 2 {
        s += 10;
    }
    if let Some(t) = changed_at {
        let age = now.duration_since(t).as_secs();
        if age < 86_400 {
            s += 20;
        } else if age < 7 * 86_400 {
            s += 10;
        }
    }
    s
}

#[allow(dead_code)]
fn _id(_f: &FeatureId) {}
