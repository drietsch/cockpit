//! The engineering timeline: events are derived by diffing this build against what the store
//! remembers (feature health, open attention) and from dated facts (ADR decisions, commits,
//! benchmark runs). Ids are stable, so re-deriving never duplicates a row.

use super::FeatureView;
use crate::ids::*;
use crate::model::*;
use crate::state::benchmark_cmp::Classification;
use crate::store::Store;
use crate::time::millis;
use jiff::Timestamp;
use std::collections::{BTreeMap, HashMap};

fn minute(t: Timestamp) -> i64 {
    millis(t) / 60_000
}

fn ev(
    kind: EventKind,
    at: Timestamp,
    feature: Option<&FeatureId>,
    subject: Option<EvidenceRef>,
    text: String,
    payload: serde_json::Value,
) -> EngineeringEvent {
    let subj = subject.as_ref().map(|s| s.id.clone()).unwrap_or_default();
    EngineeringEvent {
        id: EventId::derive(kind.as_str(), feature, &subj, minute(at)),
        kind,
        at,
        feature: feature.cloned(),
        subject,
        text,
        payload,
    }
}

pub struct EventInput<'a> {
    pub store: &'a Store,
    pub features: &'a BTreeMap<FeatureId, FeatureView>,
    pub adrs: &'a BTreeMap<AdrId, super::AdrView>,
    pub benchmarks: &'a BTreeMap<BenchmarkId, super::BenchmarkView>,
    pub commits: &'a HashMap<CommitKey, super::CommitView>,
    pub attention: &'a [AttentionItem],
    pub resolved: &'a [AttentionItem],
    pub now: Timestamp,
}

/// Derive and persist this build's events; returns the newly inserted ones.
pub fn produce(input: &EventInput) -> crate::store::Result<Vec<EngineeringEvent>> {
    let previous: HashMap<FeatureId, FeatureHealth> =
        input.store.feature_health()?.into_iter().collect();
    let first_build = previous.is_empty();
    let mut events: Vec<EngineeringEvent> = Vec::new();

    for (id, v) in input.features {
        let f = &v.feature;
        let title = f.title.clone();
        let prev = previous.get(id);
        // Implementation change: dated by the commit.
        if let (Some(k), Some(at)) = (&v.impl_commit, v.impl_committed_at) {
            let changed = prev.map(|p| p.impl_commit.as_ref() != Some(k)).unwrap_or(true);
            if changed {
                let subject =
                    input.commits.get(k).map(|c| c.commit.subject.clone()).unwrap_or_default();
                events.push(ev(
                    EventKind::FeatureChanged,
                    at,
                    Some(id),
                    Some(EvidenceRef::new(EvidenceKind::Commit, k.sha.as_str(), &subject)),
                    format!("{title}: implementation changed ({} {})", k.sha.short(), subject),
                    serde_json::json!({ "repo": k.repo, "sha": k.sha }),
                ));
            }
        }
        if first_build {
            continue;
        }
        match prev {
            None => events.push(ev(
                EventKind::NewFeature,
                input.now,
                Some(id),
                None,
                format!("{title}: new feature ({})", id),
                serde_json::json!({}),
            )),
            Some(p) if p.state != v.health.state => {
                let kind = match (p.state, v.health.state) {
                    (_, HealthState::Broken) => EventKind::FeatureBroken,
                    (HealthState::Broken, _) => EventKind::FeatureRecovered,
                    (_, HealthState::Attention) => EventKind::FeatureAttention,
                    (HealthState::Attention, HealthState::Healthy) => EventKind::FeatureRecovered,
                    (HealthState::Unknown, HealthState::Healthy) => EventKind::FeatureValidated,
                    _ => EventKind::FeatureValidated,
                };
                events.push(ev(
                    kind,
                    input.now,
                    Some(id),
                    None,
                    format!(
                        "{title}: {} → {} — {}",
                        p.state.label(),
                        v.health.state.label(),
                        v.health.headline
                    ),
                    serde_json::json!({ "from": p.state.as_str(), "to": v.health.state.as_str() }),
                ));
            }
            Some(p) => {
                // Same state, but evidence newly went stale.
                let was_stale =
                    p.reasons.iter().any(|r| r.reason == AttentionReason::StaleEvidence);
                let is_stale =
                    v.health.reasons.iter().any(|r| r.reason == AttentionReason::StaleEvidence);
                if is_stale && !was_stale {
                    events.push(ev(
                        EventKind::EvidenceStale,
                        input.now,
                        Some(id),
                        None,
                        format!("{title}: evidence predates the latest change"),
                        serde_json::json!({}),
                    ));
                }
            }
        }
    }

    // ADR decisions are dated facts: derived once, at their own dates.
    for (id, a) in input.adrs {
        let date_at = |d: jiff::civil::Date| {
            d.at(12, 0, 0, 0).to_zoned(jiff::tz::TimeZone::UTC).ok().map(|z| z.timestamp())
        };
        let accepted_same_day = a.adr.status == AdrStatus::Accepted
            && a.adr.accepted_at.map(|d| Some(d) == a.adr.date).unwrap_or(true);
        if let Some(at) = a.adr.date.filter(|_| !accepted_same_day).and_then(date_at) {
            events.push(ev(
                EventKind::NewAdr,
                at,
                None,
                Some(EvidenceRef::new(EvidenceKind::Adr, id.as_str(), &a.adr.title)),
                format!("{id}: {}", a.adr.title),
                serde_json::json!({ "status": a.adr.status.as_str() }),
            ));
        }
        if a.adr.status == AdrStatus::Accepted {
            if let Some(at) = a.adr.accepted_at.or(a.adr.date).and_then(date_at) {
                events.push(ev(
                    EventKind::AdrAccepted,
                    at,
                    None,
                    Some(EvidenceRef::new(EvidenceKind::Adr, id.as_str(), &a.adr.title)),
                    format!("{id} accepted: {}", a.adr.title),
                    serde_json::json!({}),
                ));
            }
        }
        if let Some(by) = &a.adr.superseded_by {
            let at = input.adrs.get(by).and_then(|b| b.adr.date).or(a.adr.date).and_then(date_at);
            if let Some(at) = at {
                events.push(ev(
                    EventKind::AdrSuperseded,
                    at,
                    None,
                    Some(EvidenceRef::new(EvidenceKind::Adr, id.as_str(), &a.adr.title)),
                    format!("{id} superseded by {by}"),
                    serde_json::json!({ "by": by }),
                ));
            }
        }
    }

    // Benchmark regressions and breaches, dated by the run that showed them.
    for (id, b) in input.benchmarks {
        let Some(c) = &b.comparison else { continue };
        if !matches!(c.classification, Classification::Regression | Classification::BudgetBreach) {
            continue;
        }
        let Some(run) = &b.latest else { continue };
        let feature = b.features.first().map(|(f, _)| f);
        events.push(ev(
            EventKind::RegressionDetected,
            run.timestamp.value,
            feature,
            Some(EvidenceRef::new(EvidenceKind::Benchmark, id.as_str(), b.label())),
            format!("{}: {}", b.label(), c.note),
            serde_json::json!({ "classification": c.classification.as_str(), "severity": c.severity.as_str() }),
        ));
    }

    // Attention lifecycle.
    if !first_build {
        let open_before: std::collections::HashSet<AttentionId> =
            input.store.attention_items(true)?.into_iter().map(|a| a.id).collect();
        for a in input.attention {
            if !open_before.contains(&a.id) && a.severity != Severity::Informational {
                events.push(ev(
                    EventKind::AttentionOpened,
                    input.now,
                    a.feature.as_ref(),
                    Some(EvidenceRef::new(EvidenceKind::Attention, a.id.as_str(), &a.title)),
                    format!("{}{}", a.feature_title.as_ref().map(|t| format!("{t}: ")).unwrap_or_default(), a.summary),
                    serde_json::json!({ "reason": a.reason.as_str(), "severity": a.severity.as_str() }),
                ));
            }
        }
    }
    for a in input.resolved {
        if a.severity == Severity::Informational {
            continue;
        }
        events.push(ev(
            EventKind::AttentionResolved,
            input.now,
            a.feature.as_ref(),
            Some(EvidenceRef::new(EvidenceKind::Attention, a.id.as_str(), &a.title)),
            format!(
                "{}{} — resolved",
                a.feature_title.as_ref().map(|t| format!("{t}: ")).unwrap_or_default(),
                a.summary
            ),
            serde_json::json!({ "reason": a.reason.as_str() }),
        ));
    }

    let mut inserted = Vec::new();
    for e in events {
        if input.store.insert_event(&e)? {
            inserted.push(e);
        }
    }
    Ok(inserted)
}
