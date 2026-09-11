//! Build a `ProjectState` from the store: link, evaluate, rank, persist derived rows.

use super::*;
use crate::config::Located;
use crate::git::CommitGraph;
use crate::link::commit_link::link_commit;
use crate::link::{FeatureIdIndex, PathOwnership};
use crate::store::Store;
use crate::time::{millis, now as time_now};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkComparison {
    pub baseline_run: Option<BenchmarkRunId>,
    pub baseline_value: Option<f64>,
    pub absolute_change: Option<f64>,
    pub relative_change: Option<f64>,
    pub classification: String,
    pub confidence: Confidence,
    pub note: String,
}

pub struct BuildInput<'a> {
    pub store: &'a Store,
    pub located: &'a Located,
    pub project: Project,
    pub previous: Option<&'a ProjectState>,
    pub last_scan_duration_ms: Option<u64>,
}

pub fn build(input: BuildInput) -> crate::store::Result<ProjectState> {
    let now = time_now();
    let store = input.store;
    let cfg = &input.located.config;
    let project = input.project;

    let features = store.features()?;
    let adrs = store.adrs()?;
    let all_commits = store.all_commits()?;
    let warnings = store.warnings()?;

    let idx = FeatureIdIndex::build(&features, cfg.features.id_pattern.as_deref());
    let ownership = PathOwnership::build(&features);
    let allow_bare = cfg.features.id_pattern.is_some() || true;

    // Implementation-path exclusions: docs, evidence, the feature/ADR dirs themselves.
    let mut excl = GlobSetBuilder::new();
    for g in &cfg.git.implementation_paths_exclude {
        if let Ok(g) = Glob::new(g) {
            excl.add(g);
        }
    }
    for src in input.located.evidence_sources() {
        for p in src.paths() {
            if let Ok(g) = Glob::new(&p) {
                excl.add(g);
            }
        }
    }
    for dir in std::iter::once(cfg.features.path.clone()).chain(cfg.adrs.path.items()) {
        if let Ok(g) = Glob::new(&format!("{}/**", dir.trim_end_matches('/'))) {
            excl.add(g);
        }
    }
    let excluded: GlobSet = excl.build().unwrap_or_else(|_| GlobSet::empty());

    // Commit graph + commit views + commit→feature links.
    let mut graph = CommitGraph::new();
    let mut commits: HashMap<CommitKey, CommitView> = HashMap::new();
    let mut relationships: Vec<Relationship> = Vec::new();
    let mut feature_commits: HashMap<FeatureId, Vec<CommitLink>> = HashMap::new();
    let mut impl_candidates: HashMap<FeatureId, Vec<(CommitKey, Timestamp)>> = HashMap::new();
    let adr_re = regex::Regex::new(r"\bADR[-\s_]?(\d{1,6})\b").unwrap();
    let mut adr_citations: HashMap<u32, Vec<CommitKey>> = HashMap::new();
    for c in &all_commits {
        graph.insert(c.key.sha.clone(), c.parents.clone(), c.committed_at);
        let rels = link_commit(c, &idx, &ownership, allow_bare);
        let mut feats: Vec<(FeatureId, RelationOrigin)> = Vec::new();
        for r in &rels {
            if let EntityRef::Feature(fid) = &r.to {
                feats.push((fid.clone(), r.origin.clone()));
                feature_commits.entry(fid.clone()).or_default().push(CommitLink {
                    key: c.key.clone(),
                    origin: r.origin.clone(),
                    committed_at: c.committed_at,
                });
                // Implementation change = explicit reference, or a matching file outside the
                // excluded (docs/evidence) set.
                let is_impl = r.origin.is_explicit()
                    || c.files.iter().any(|f| {
                        !excluded.is_match(f.path.as_str())
                            && ownership.owners_of(&f.path).iter().any(|(id, _)| *id == fid)
                    });
                if is_impl {
                    impl_candidates
                        .entry(fid.clone())
                        .or_default()
                        .push((c.key.clone(), c.committed_at));
                }
            }
        }
        relationships.extend(rels);
        let text = format!("{}\n{}", c.subject, c.body);
        let mut adr_refs: Vec<AdrId> = Vec::new();
        for m in adr_re.captures_iter(&text) {
            if let Ok(n) = m[1].parse::<u32>() {
                adr_citations.entry(n).or_default().push(c.key.clone());
                let id = AdrId::new(format!("ADR-{}", &m[1]));
                if !adr_refs.contains(&id) {
                    adr_refs.push(id);
                }
            }
        }
        commits.insert(c.key.clone(), CommitView { commit: c.clone(), features: feats, adr_refs });
    }
    let mut recent: Vec<(CommitKey, Timestamp)> =
        commits.values().map(|c| (c.commit.key.clone(), c.commit.committed_at)).collect();
    recent.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.sha.cmp(&b.0.sha)));
    let recent_commits: Vec<CommitKey> = recent.into_iter().take(500).map(|(k, _)| k).collect();
    let mut tags: Vec<(RepoId, String, Sha)> = Vec::new();
    for c in commits.values() {
        for t in c.commit.tags() {
            tags.push((c.commit.key.repo.clone(), t, c.commit.key.sha.clone()));
        }
    }
    tags.sort_by(|a, b| a.1.cmp(&b.1));

    // ADR index and links.
    let mut adr_by_number: HashMap<u32, AdrId> = HashMap::new();
    let mut adr_ids: HashSet<AdrId> = HashSet::new();
    for a in &adrs {
        adr_ids.insert(a.id.clone());
        if let Some(n) = a.number {
            adr_by_number.entry(n).or_insert(a.id.clone());
        }
    }
    let resolve_adr = |id: &AdrId| -> Option<AdrId> {
        if adr_ids.contains(id) {
            return Some(id.clone());
        }
        id.number().and_then(|n| adr_by_number.get(&n).cloned())
    };
    let mut unresolved: Vec<UnresolvedRef> = Vec::new();
    let mut feature_adrs: HashMap<FeatureId, Vec<AdrId>> = HashMap::new();
    let mut adr_features: HashMap<AdrId, Vec<FeatureId>> = HashMap::new();
    for f in &features {
        for a in &f.adr_ids {
            match resolve_adr(a) {
                Some(rid) => {
                    feature_adrs.entry(f.id.clone()).or_default().push(rid.clone());
                    adr_features.entry(rid.clone()).or_default().push(f.id.clone());
                    relationships.push(Relationship {
                        from: EntityRef::Adr(rid),
                        to: EntityRef::Feature(f.id.clone()),
                        kind: RelationKind::AdrGoverns,
                        origin: RelationOrigin::Explicit(ExplicitSource::Frontmatter),
                        confidence: Confidence::High,
                    });
                }
                None => unresolved.push(UnresolvedRef {
                    from: EntityRef::Feature(f.id.clone()),
                    raw: a.0.clone(),
                    target_kind: "adr".into(),
                    note: Some(format!("{} references {}, which does not exist", f.id, a)),
                }),
            }
        }
        for d in &f.dependencies {
            if idx.contains(d) {
                relationships.push(Relationship {
                    from: EntityRef::Feature(f.id.clone()),
                    to: EntityRef::Feature(d.clone()),
                    kind: RelationKind::FeatureDependsOn,
                    origin: RelationOrigin::Explicit(ExplicitSource::Frontmatter),
                    confidence: Confidence::High,
                });
            } else {
                unresolved.push(UnresolvedRef {
                    from: EntityRef::Feature(f.id.clone()),
                    raw: d.0.clone(),
                    target_kind: "feature".into(),
                    note: Some(format!("{} depends on {}, which does not exist", f.id, d)),
                });
            }
        }
        if let Some(s) = &f.superseded_by {
            if idx.contains(s) {
                relationships.push(Relationship {
                    from: EntityRef::Feature(f.id.clone()),
                    to: EntityRef::Feature(s.clone()),
                    kind: RelationKind::FeatureSupersedes,
                    origin: RelationOrigin::Explicit(ExplicitSource::Frontmatter),
                    confidence: Confidence::High,
                });
            }
        }
    }
    for a in &adrs {
        for fid in &a.feature_ids {
            if idx.contains(fid) {
                if !adr_features.entry(a.id.clone()).or_default().contains(fid) {
                    adr_features.get_mut(&a.id).unwrap().push(fid.clone());
                    feature_adrs.entry(fid.clone()).or_default().push(a.id.clone());
                    relationships.push(Relationship {
                        from: EntityRef::Adr(a.id.clone()),
                        to: EntityRef::Feature(fid.clone()),
                        kind: RelationKind::AdrGoverns,
                        origin: RelationOrigin::Explicit(ExplicitSource::Frontmatter),
                        confidence: Confidence::High,
                    });
                }
            } else {
                unresolved.push(UnresolvedRef {
                    from: EntityRef::Adr(a.id.clone()),
                    raw: fid.0.clone(),
                    target_kind: "feature".into(),
                    note: Some(format!("{} lists feature {}, which does not exist", a.id, fid)),
                });
            }
        }
        for tok in &a.mentioned_tokens {
            if let Some(fid) = idx.resolve(tok) {
                if !adr_features.entry(a.id.clone()).or_default().contains(&fid) {
                    adr_features.get_mut(&a.id).unwrap().push(fid.clone());
                    feature_adrs.entry(fid.clone()).or_default().push(a.id.clone());
                    relationships.push(Relationship {
                        from: EntityRef::Adr(a.id.clone()),
                        to: EntityRef::Feature(fid),
                        kind: RelationKind::AdrGoverns,
                        origin: RelationOrigin::Inferred(InferredSource::AdrMention),
                        confidence: Confidence::Low,
                    });
                }
            }
        }
        if let Some(s) = &a.superseded_by {
            match resolve_adr(s) {
                Some(rid) => relationships.push(Relationship {
                    from: EntityRef::Adr(a.id.clone()),
                    to: EntityRef::Adr(rid),
                    kind: RelationKind::AdrSupersedes,
                    origin: RelationOrigin::Explicit(ExplicitSource::AdrBody),
                    confidence: Confidence::High,
                }),
                None => unresolved.push(UnresolvedRef {
                    from: EntityRef::Adr(a.id.clone()),
                    raw: s.0.clone(),
                    target_kind: "adr".into(),
                    note: Some(format!("{} is superseded by {}, which does not exist", a.id, s)),
                }),
            }
        }
    }
    let mut adr_views: BTreeMap<AdrId, AdrView> = BTreeMap::new();
    for a in adrs {
        let citing = a.number.and_then(|n| adr_citations.get(&n)).cloned().unwrap_or_default();
        let mut feats = adr_features.get(&a.id).cloned().unwrap_or_default();
        feats.sort();
        feats.dedup();
        adr_views.insert(
            a.id.clone(),
            AdrView {
                adr: a,
                features: feats,
                citing_commits: citing.into_iter().take(20).collect(),
            },
        );
    }

    // Feature views with (M0) health.
    let mut feature_views: BTreeMap<FeatureId, FeatureView> = BTreeMap::new();
    for f in features {
        let mut links = feature_commits.remove(&f.id).unwrap_or_default();
        links.sort_by(|a, b| b.committed_at.cmp(&a.committed_at));
        links.truncate(200);
        let impl_commit = impl_candidates.remove(&f.id).and_then(|c| {
            graph
                .newest(c.iter().map(|(k, _)| &k.sha))
                .and_then(|sha| c.into_iter().find(|(k, _)| k.sha == sha))
        });
        let (impl_key, impl_at) = match impl_commit {
            Some((k, t)) => (Some(k), Some(t)),
            None => (None, None),
        };
        let mut adr_list = feature_adrs.remove(&f.id).unwrap_or_default();
        adr_list.sort();
        adr_list.dedup();
        let health = crate::state::health::evaluate(
            &f,
            &crate::state::health::Evidence::default(),
            impl_key.as_ref(),
            impl_at,
            &unresolved,
            cfg,
            now,
        );
        feature_views.insert(
            f.id.clone(),
            FeatureView {
                feature: f,
                health,
                impl_commit: impl_key,
                impl_committed_at: impl_at,
                commits: links,
                adrs: adr_list,
                tests: vec![],
                benchmarks: vec![],
                visuals: vec![],
            },
        );
    }

    // Attention: feature reasons + project-level items, merged with persisted first_seen.
    let attention =
        crate::state::attention::collect(&feature_views, &warnings, &unresolved, store, now)?;

    // Counts, headline.
    let mut counts = StateCounts {
        features: feature_views.len(),
        adrs: adr_views.len(),
        commits: commits.len(),
        warnings: warnings.len(),
        ..Default::default()
    };
    for v in feature_views.values() {
        match v.health.state {
            HealthState::Healthy => counts.healthy += 1,
            HealthState::Attention => counts.attention += 1,
            HealthState::Broken => counts.broken += 1,
            HealthState::Unknown => counts.unknown += 1,
        }
    }
    for a in adr_views.values() {
        match a.adr.status {
            AdrStatus::Accepted => counts.adrs_accepted += 1,
            AdrStatus::Proposed => counts.adrs_proposed += 1,
            _ => {}
        }
    }
    counts.attention_items = attention.len();
    let (overall, headline) = headline(&counts);

    // Persist derived rows.
    store.batch_begin()?;
    store.replace_relationships(&relationships, &unresolved)?;
    for (id, v) in &feature_views {
        store.upsert_feature_health(id, &v.health)?;
    }
    for a in &attention {
        store.upsert_attention(a)?;
    }
    store.batch_commit()?;

    let generation = input.previous.map(|p| p.generation + 1).unwrap_or(1);
    Ok(ProjectState {
        project,
        generation,
        generated_at: now,
        last_scan_at: store.meta_timestamp("last_scan_at")?,
        last_scan_duration_ms: input.last_scan_duration_ms,
        counts,
        headline,
        overall,
        features: feature_views,
        adrs: adr_views,
        commits,
        recent_commits,
        tags,
        attention,
        events: store.events(500)?,
        unresolved,
        warnings,
        suites: vec![],
        tests: BTreeMap::new(),
        benchmarks: BTreeMap::new(),
        visuals: BTreeMap::new(),
        environments: HashMap::new(),
        artifacts: HashMap::new(),
        sources: summarize_sources(input.located),
    })
}

pub fn headline(c: &StateCounts) -> (HealthState, String) {
    if c.features == 0 {
        return (HealthState::Unknown, "No features defined yet".into());
    }
    if c.broken > 0 {
        let s = format!(
            "{} broken feature{}, {} need{} attention",
            c.broken,
            if c.broken == 1 { "" } else { "s" },
            c.attention,
            if c.attention == 1 { "s" } else { "" }
        );
        return (HealthState::Broken, s);
    }
    if c.attention > 0 {
        let s = format!(
            "Healthy with {} item{} needing attention",
            c.attention,
            if c.attention == 1 { "" } else { "s" }
        );
        return (HealthState::Attention, s);
    }
    if c.healthy == 0 {
        return (HealthState::Unknown, "No evidence yet".into());
    }
    if c.unknown > 0 {
        return (
            HealthState::Healthy,
            format!(
                "Healthy · {} feature{} without evidence",
                c.unknown,
                if c.unknown == 1 { "" } else { "s" }
            ),
        );
    }
    (HealthState::Healthy, "Healthy".into())
}

pub fn summarize_sources(located: &Located) -> Vec<SourceSummary> {
    located
        .evidence_sources()
        .iter()
        .enumerate()
        .map(|(i, s)| SourceSummary {
            id: s.id.clone().unwrap_or_else(|| format!("auto:{}:{i}", s.format.as_str())),
            title: s.title.clone(),
            format: s.format.as_str().into(),
            kind: s.default_kind().into(),
            patterns: s.paths(),
            component: s.component.clone(),
            enabled: s.enabled,
            files_matched: 0,
            runs: 0,
            last_seen: None,
            github: s
                .github
                .as_ref()
                .map(|g| format!("{} · {} · {}", g.repo, g.workflow, g.artifact)),
            note: s.note.clone(),
        })
        .collect()
}

#[allow(dead_code)]
fn _ms(t: Timestamp) -> i64 {
    millis(t)
}
