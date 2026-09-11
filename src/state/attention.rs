//! Attention items: generated from feature reasons plus project-level findings, merged with
//! persisted rows so `first_seen` survives rescans.

use super::FeatureView;
use crate::ids::{AttentionId, FeatureId};
use crate::model::*;
use crate::store::Store;
use jiff::Timestamp;
use std::collections::{BTreeMap, HashMap};

/// Open items, ranked, plus the previously open items this build resolved.
pub struct Collected {
    pub items: Vec<AttentionItem>,
    pub resolved: Vec<AttentionItem>,
}

pub fn collect(
    features: &BTreeMap<FeatureId, FeatureView>,
    warnings: &[ImportWarning],
    unresolved: &[UnresolvedRef],
    extra: Vec<AttentionItem>,
    store: &Store,
    now: Timestamp,
) -> crate::store::Result<Collected> {
    let mut items: Vec<AttentionItem> = extra;
    for v in features.values() {
        for r in &v.health.reasons {
            let id = AttentionId::derive(r.reason.as_str(), Some(&v.feature.id), &r.subject);
            items.push(AttentionItem {
                id,
                reason: r.reason,
                severity: r.severity,
                status: AttentionStatus::Open,
                feature: Some(v.feature.id.clone()),
                feature_title: Some(v.feature.title.clone()),
                subject: r.subject.clone(),
                title: r.reason.label().to_string(),
                summary: r.text.clone(),
                why_it_matters: why(r.reason, &v.feature),
                evidence: r.evidence.clone(),
                confidence: r.confidence,
                score: 0,
                first_seen: now,
                last_seen: now,
                resolved_at: None,
                commit: r.commit.clone().or_else(|| v.impl_commit.clone()),
                payload: serde_json::json!({ "category": r.category.map(|c| c.as_str()) }),
            });
        }
    }
    for w in warnings.iter().filter(|w| w.fatal) {
        items.push(AttentionItem {
            id: AttentionId::derive("import_error", None, w.path.as_str()),
            reason: AttentionReason::ImportError,
            severity: Severity::Low,
            status: AttentionStatus::Open,
            feature: None,
            feature_title: None,
            subject: w.path.as_str().to_string(),
            title: "Import error".into(),
            summary: format!("{}: {}", w.path, w.message),
            why_it_matters: "Evidence in this file is not reaching Cockpit, so the state it would support is missing.".into(),
            evidence: vec![],
            confidence: Confidence::High,
            score: 0,
            first_seen: now,
            last_seen: now,
            resolved_at: None,
            commit: None,
            payload: serde_json::json!({ "location": w.location, "importer": w.importer }),
        });
    }
    for u in unresolved.iter().filter(|u| !matches!(u.from, EntityRef::Feature(_))) {
        items.push(AttentionItem {
            id: AttentionId::derive(
                "unresolved_reference",
                None,
                &format!("{}:{}", u.from.id_str(), u.raw),
            ),
            reason: AttentionReason::UnresolvedReference,
            severity: Severity::Low,
            status: AttentionStatus::Open,
            feature: None,
            feature_title: None,
            subject: u.raw.clone(),
            title: "Unresolved reference".into(),
            summary: u
                .note
                .clone()
                .unwrap_or_else(|| format!("{} → {} does not resolve", u.from.id_str(), u.raw)),
            why_it_matters:
                "A dangling reference means the graph Cockpit reasons over is incomplete.".into(),
            evidence: vec![],
            confidence: Confidence::High,
            score: 0,
            first_seen: now,
            last_seen: now,
            resolved_at: None,
            commit: None,
            payload: serde_json::json!({ "from": u.from.id_str(), "target_kind": u.target_kind }),
        });
    }

    // Merge with persisted rows: keep first_seen; resolve what disappeared.
    let persisted: HashMap<AttentionId, AttentionItem> =
        store.attention_items(false)?.into_iter().map(|a| (a.id.clone(), a)).collect();
    for it in items.iter_mut() {
        if let Some(prev) = persisted.get(&it.id) {
            it.first_seen = prev.first_seen;
        }
        it.score = score(it, features, now);
    }
    let live: std::collections::HashSet<AttentionId> = items.iter().map(|i| i.id.clone()).collect();
    let mut resolved_items = Vec::new();
    for (id, prev) in persisted {
        if !live.contains(&id) && prev.status == AttentionStatus::Open {
            let mut resolved = prev.clone();
            resolved.status = AttentionStatus::Resolved;
            resolved.resolved_at = Some(now);
            store.upsert_attention(&resolved)?;
            resolved_items.push(resolved);
        }
    }
    items.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.first_seen.cmp(&a.first_seen))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(Collected { items, resolved: resolved_items })
}

fn score(it: &AttentionItem, features: &BTreeMap<FeatureId, FeatureView>, now: Timestamp) -> i32 {
    let mut s = 0;
    if let Some(f) = it.feature.as_ref().and_then(|id| features.get(id)) {
        s += match f.feature.importance {
            Importance::Critical => 50,
            Importance::High => 30,
            Importance::Normal => 15,
            Importance::Low => 5,
        };
        if f.health.state == HealthState::Broken {
            s += 40;
        }
        if f.health.reasons.len() >= 2 {
            s += 10;
        }
    }
    s += match it.severity {
        Severity::Critical => 40,
        Severity::High => 30,
        Severity::Medium => 20,
        Severity::Low => 10,
        Severity::Informational => 0,
    };
    let age = now.duration_since(it.first_seen).as_secs();
    if age < 86_400 {
        s += 20;
    } else if age < 7 * 86_400 {
        s += 10;
    }
    s += match it.confidence {
        Confidence::High => 10,
        Confidence::Medium => 5,
        Confidence::Low => 0,
    };
    s
}

fn why(reason: AttentionReason, feature: &Feature) -> String {
    let imp = feature.importance.as_str();
    match reason {
        AttentionReason::MissingRequiredTest => {
            format!("A {imp} feature without the required evidence cannot be trusted to work.")
        }
        AttentionReason::UnresolvedReference => {
            "The feature points at something that does not exist; the graph is incomplete.".into()
        }
        AttentionReason::NoEvidence => {
            "Without evidence Cockpit cannot say whether this feature works.".into()
        }
        AttentionReason::TestFailed => {
            "A failing test means a stated expectation is not met.".into()
        }
        AttentionReason::StaleEvidence => {
            "The implementation changed after the last validation; the evidence may no longer hold."
                .into()
        }
        AttentionReason::BenchmarkRegression => {
            "Performance moved in the wrong direction after a change.".into()
        }
        AttentionReason::BenchmarkBudgetBreach => {
            "A performance budget is a contract, and it is violated.".into()
        }
        _ => reason.label().to_string(),
    }
}
