//! Test → feature linkage in priority tiers: explicit ids the test carries, `[[test_map]]`
//! entries, path ownership, and finally the unlinked bucket (never dropped).

use super::{FeatureIdIndex, PathOwnership};
use crate::config::TestMapEntry;
use crate::ids::{FeatureId, TestId};
use crate::model::*;
use std::collections::HashMap;

pub struct TestLinks {
    pub relationships: Vec<Relationship>,
    pub unresolved: Vec<UnresolvedRef>,
    /// Tests with no feature, per suite id.
    pub unlinked: HashMap<String, Vec<TestId>>,
    pub by_test: HashMap<TestId, Vec<(FeatureId, RelationOrigin)>>,
}

pub fn link_tests(
    tests: &[Test],
    suites: &[Suite],
    idx: &FeatureIdIndex,
    ownership: &PathOwnership,
    map: &[TestMapEntry],
) -> TestLinks {
    let suite_by_id: HashMap<&str, &Suite> = suites.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut relationships = Vec::new();
    let mut unresolved = Vec::new();
    let mut unlinked: HashMap<String, Vec<TestId>> = HashMap::new();
    let mut by_test: HashMap<TestId, Vec<(FeatureId, RelationOrigin)>> = HashMap::new();
    for t in tests {
        let mut links: Vec<(FeatureId, RelationOrigin, Confidence)> = Vec::new();
        // Tier 1: explicit ids carried by the test.
        for raw in &t.declared_feature_ids {
            if let Some(flat) = raw.strip_prefix("flat:") {
                match idx.resolve_flat(flat) {
                    Ok(id) => links.push((
                        id,
                        RelationOrigin::Explicit(ExplicitSource::TestName),
                        Confidence::High,
                    )),
                    Err(amb) => unresolved.push(UnresolvedRef {
                        from: EntityRef::Test(t.id.clone()),
                        raw: raw.clone(),
                        target_kind: "feature".into(),
                        note: Some(if amb.is_empty() {
                            format!("__feat__{flat} matches no feature id")
                        } else {
                            format!(
                                "__feat__{flat} is ambiguous between {}",
                                amb.iter().map(|a| a.as_str()).collect::<Vec<_>>().join(", ")
                            )
                        }),
                    }),
                }
                continue;
            }
            match idx.resolve(raw) {
                Some(id) => {
                    let origin = if t.tags.iter().any(|tg| tg.contains(raw)) || raw.contains(':') {
                        ExplicitSource::TestTag
                    } else {
                        ExplicitSource::Interchange
                    };
                    links.push((id, RelationOrigin::Explicit(origin), Confidence::High));
                }
                None => unresolved.push(UnresolvedRef {
                    from: EntityRef::Test(t.id.clone()),
                    raw: raw.clone(),
                    target_kind: "feature".into(),
                    note: Some(format!(
                        "test {} references feature {raw}, which does not exist",
                        t.name
                    )),
                }),
            }
        }
        // Tier 2: test map.
        if links.is_empty() {
            let suite = suite_by_id.get(t.suite.as_str());
            let source_id = suite.map(|s| s.source.as_str()).unwrap_or("");
            let suite_name = suite.map(|s| s.name.as_str()).unwrap_or("");
            for e in map.iter().filter(|e| super::test_map::matches(e, t, source_id, suite_name)) {
                for f in &e.features {
                    match idx.resolve(f) {
                        Some(id) => {
                            if !links.iter().any(|(x, _, _)| x == &id) {
                                links.push((
                                    id,
                                    RelationOrigin::Explicit(ExplicitSource::TestMap),
                                    Confidence::High,
                                ));
                            }
                        }
                        None => unresolved.push(UnresolvedRef {
                            from: EntityRef::Test(t.id.clone()),
                            raw: f.clone(),
                            target_kind: "feature".into(),
                            note: Some(format!(
                                "test map entry references feature {f}, which does not exist"
                            )),
                        }),
                    }
                }
            }
        }
        // Tier 3: path ownership.
        if links.is_empty() {
            if let Some(p) = &t.source_path {
                for (id, glob) in ownership.owners_of(p) {
                    links.push((
                        id.clone(),
                        RelationOrigin::Inferred(InferredSource::PathMatch {
                            glob: glob.to_string(),
                        }),
                        Confidence::Medium,
                    ));
                }
            }
        }
        if links.is_empty() {
            unlinked.entry(t.suite.0.clone()).or_default().push(t.id.clone());
            continue;
        }
        for (id, origin, confidence) in links {
            relationships.push(Relationship {
                from: EntityRef::Test(t.id.clone()),
                to: EntityRef::Feature(id.clone()),
                kind: RelationKind::TestCovers,
                origin: origin.clone(),
                confidence,
            });
            by_test.entry(t.id.clone()).or_default().push((id, origin));
        }
    }
    TestLinks { relationships, unresolved, unlinked, by_test }
}
