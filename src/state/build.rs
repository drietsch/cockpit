//! Build a `ProjectState` from the store: link, evaluate, rank, persist derived rows.

use super::*;
use crate::config::Located;
use crate::git::CommitGraph;
use crate::link::commit_link::link_commit;
use crate::link::{FeatureIdIndex, PathOwnership};
use crate::store::Store;
use crate::time::{millis, now as time_now};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::collections::{BTreeMap, HashMap, HashSet};

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
    let _ = &warnings;

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
                let e = adr_citations.entry(n).or_default();
                if !e.contains(&c.key) {
                    e.push(c.key.clone());
                }
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

    // Evidence: suites, tests, recent runs, visuals; per-test history; test → feature links.
    let suites = store.suites()?;
    let tests_all = store.tests()?;
    let recent_runs = store.recent_test_runs(cfg.health.flaky_window as usize)?;
    // A test is current when its newest run is the newest run of the evidence file it came
    // from, and that file still exists. Renamed or removed tests keep their history but leave
    // the matrix; the same goes for a suite whose report name changed.
    let all_runs = store.runs()?;
    let present_files = store.scan_files()?;
    let newest_for_path: HashMap<(crate::paths::RelPath, String), RunId> =
        store.newest_run_per_path()?;
    let run_path: HashMap<RunId, (crate::paths::RelPath, String)> = all_runs
        .iter()
        .map(|r| (r.id.clone(), (r.provenance.source_path.clone(), r.source.as_str().to_string())))
        .collect();
    let mut latest_run_of_test: HashMap<TestId, (Timestamp, RunId)> = HashMap::new();
    for tr in &recent_runs {
        let e = latest_run_of_test
            .entry(tr.test.clone())
            .or_insert((tr.timestamp.value, tr.run.clone()));
        if tr.timestamp.value > e.0 {
            *e = (tr.timestamp.value, tr.run.clone());
        }
    }
    // Evidence from a source that is no longer configured is history, not state.
    let configured_sources: HashSet<String> = input
        .located
        .evidence_sources()
        .iter()
        .enumerate()
        .map(|(i, s)| s.id.clone().unwrap_or_else(|| format!("auto:{}:{i}", s.format.as_str())))
        .collect();
    let run_is_current = |run: &RunId| -> bool {
        let Some(key) = run_path.get(run) else { return true };
        if !configured_sources.contains(&key.1) {
            return false;
        }
        if !present_files.contains_key(&key.0) && !key.0.as_str().starts_with("external/") {
            return false;
        }
        newest_for_path.get(key).map(|newest| newest == run).unwrap_or(true)
    };
    let is_current = |t: &Test| -> bool {
        let Some((_, run)) = latest_run_of_test.get(&t.id) else { return false };
        run_is_current(run)
    };
    let tests_raw: Vec<Test> = tests_all.into_iter().filter(is_current).collect();
    let visuals_raw = store.recent_visuals()?;
    let artifacts_raw = store.artifacts()?;
    let environments_raw = store.environments()?;
    let environments_by_id: HashMap<EnvironmentId, EnvironmentFingerprint> =
        environments_raw.iter().map(|e| (e.id.clone(), e.clone())).collect();
    let mut runs_by_test: HashMap<TestId, Vec<TestRun>> = HashMap::new();
    for tr in recent_runs {
        runs_by_test.entry(tr.test.clone()).or_default().push(tr);
    }
    for v in runs_by_test.values_mut() {
        v.sort_by(|a, b| b.timestamp.value.cmp(&a.timestamp.value).then_with(|| b.id.cmp(&a.id)));
    }
    let (test_map, map_warnings) = crate::link::test_map::load(input.located);
    let mut warnings = warnings;
    for w in map_warnings {
        warnings.push(ImportWarning {
            path: crate::paths::RelPath::new("cockpit.toml"),
            importer: "test-map".into(),
            message: w,
            location: None,
            fatal: false,
        });
    }
    let links =
        crate::link::test_link::link_tests(&tests_raw, &suites, &idx, &ownership, &test_map);
    relationships.extend(links.relationships.iter().cloned());
    unresolved.extend(links.unresolved.iter().cloned());
    let mut tests: BTreeMap<TestId, TestView> = BTreeMap::new();
    let mut feature_tests: HashMap<FeatureId, Vec<TestId>> = HashMap::new();
    for t in tests_raw {
        let history = runs_by_test.remove(&t.id).unwrap_or_default();
        let h = crate::state::test_history::analyse(&history, &cfg.health);
        let feats = links.by_test.get(&t.id).cloned().unwrap_or_default();
        for (f, _) in &feats {
            feature_tests.entry(f.clone()).or_default().push(t.id.clone());
        }
        tests.insert(
            t.id.clone(),
            TestView {
                test: t,
                features: feats,
                latest: history.first().cloned(),
                history,
                flaky: h.flaky,
                flaky_text: h.flaky_text,
                duration_trend: h.duration_trend,
            },
        );
    }
    // Visual checks: latest + previous per check, linked through their test (or declared ids).
    let mut visuals: BTreeMap<VisualCheckId, VisualView> = BTreeMap::new();
    let mut by_check: HashMap<VisualCheckId, Vec<VisualEvidence>> = HashMap::new();
    for v in visuals_raw {
        by_check.entry(v.check.clone()).or_default().push(v);
    }
    let mut feature_visuals: HashMap<FeatureId, Vec<VisualCheckId>> = HashMap::new();
    for (check, mut evs) in by_check {
        evs.sort_by_key(|e| std::cmp::Reverse(e.timestamp.value));
        let latest = evs.remove(0);
        let mut feats: Vec<(FeatureId, RelationOrigin)> = latest
            .test
            .as_ref()
            .and_then(|t| tests.get(t))
            .map(|t| t.features.clone())
            .unwrap_or_default();
        for raw in &latest.declared_feature_ids {
            if let Some(id) = idx.resolve(raw) {
                if !feats.iter().any(|(f, _)| f == &id) {
                    feats.push((id, RelationOrigin::Explicit(ExplicitSource::Interchange)));
                }
            }
        }
        for (f, _) in &feats {
            feature_visuals.entry(f.clone()).or_default().push(check.clone());
        }
        visuals.insert(
            check,
            VisualView { latest, previous: evs.into_iter().next(), features: feats },
        );
    }
    // Commit graphs per repo for freshness.
    let mut graphs: HashMap<RepoId, CommitGraph> = HashMap::new();
    for c in &all_commits {
        graphs.entry(c.key.repo.clone()).or_default().insert(
            c.key.sha.clone(),
            c.parents.clone(),
            c.committed_at,
        );
    }
    let max_age = cfg.health.max_evidence_age();

    // Benchmarks: one series per benchmark (all environments), compared and classified; linked
    // to features by declared ids (document, per-metric, or the evidence source's defaults).
    let benchmarks_raw = store.benchmarks()?;
    let bench_runs = store.recent_benchmark_runs(cfg.performance.history_limit)?;
    let mut runs_by_bench: HashMap<BenchmarkId, Vec<BenchmarkRun>> = HashMap::new();
    for r in bench_runs {
        runs_by_bench.entry(r.benchmark.clone()).or_default().push(r);
    }
    let commit_files: HashMap<CommitKey, Vec<String>> = all_commits
        .iter()
        .map(|c| (c.key.clone(), c.files.iter().map(|f| f.path.as_str().to_string()).collect()))
        .collect();
    let feature_globs: HashMap<FeatureId, Vec<String>> = feature_paths_by_id(store)?;
    let mut benchmarks: BTreeMap<BenchmarkId, BenchmarkView> = BTreeMap::new();
    let mut feature_benchmarks: HashMap<FeatureId, Vec<BenchmarkId>> = HashMap::new();
    for b in benchmarks_raw {
        // Same currency rule as tests: the series is current while its newest run is the newest
        // run of the file it came from (a renamed source leaves its old series in history).
        let current = runs_by_bench
            .get(&b.id)
            .and_then(|rs| rs.iter().max_by_key(|r| (r.timestamp.value, r.id.clone())))
            .map(|latest| run_is_current(&latest.run))
            .unwrap_or(false);
        if !current {
            continue;
        }
        let mut feats: Vec<(FeatureId, RelationOrigin)> = Vec::new();
        for raw in &b.declared_feature_ids {
            match idx.resolve(raw) {
                Some(id) => {
                    if !feats.iter().any(|(f, _)| f == &id) {
                        relationships.push(Relationship {
                            from: EntityRef::Benchmark(b.id.clone()),
                            to: EntityRef::Feature(id.clone()),
                            kind: RelationKind::BenchmarkMeasures,
                            origin: RelationOrigin::Explicit(ExplicitSource::Interchange),
                            confidence: Confidence::High,
                        });
                        feats.push((id, RelationOrigin::Explicit(ExplicitSource::Interchange)));
                    }
                }
                None => unresolved.push(UnresolvedRef {
                    from: EntityRef::Benchmark(b.id.clone()),
                    raw: raw.clone(),
                    target_kind: "feature".into(),
                    note: Some(format!(
                        "benchmark {} names feature {raw}, which does not exist",
                        b.name
                    )),
                }),
            }
        }
        let mut owned = GlobSetBuilder::new();
        for (f, _) in &feats {
            for g in feature_globs.get(f).into_iter().flatten() {
                if let Ok(g) = Glob::new(g) {
                    owned.add(g);
                }
            }
        }
        let owned = owned.build().unwrap_or_else(|_| GlobSet::empty());
        let mut history = runs_by_bench.remove(&b.id).unwrap_or_default();
        history.sort_by(|a, c| {
            c.timestamp.value.cmp(&a.timestamp.value).then_with(|| c.id.cmp(&a.id))
        });
        let comparison =
            crate::state::benchmark_cmp::compare(&crate::state::benchmark_cmp::CompareInput {
                benchmark: &b,
                runs: &history,
                cfg: &cfg.performance,
                environments: &environments_by_id,
                tags: &tags,
                graphs: &graphs,
                owned_paths: &owned,
                commit_files: &commit_files,
            });
        for (f, _) in &feats {
            feature_benchmarks.entry(f.clone()).or_default().push(b.id.clone());
        }
        let latest = history
            .iter()
            .find(|r| r.measurement.status == MeasurementStatus::Ok)
            .or(history.first())
            .cloned();
        let environment_label = latest
            .as_ref()
            .and_then(|r| environments_by_id.get(&r.environment))
            .map(|e| e.label.clone())
            .unwrap_or_default();
        benchmarks.insert(
            b.id.clone(),
            BenchmarkView {
                benchmark: b,
                features: feats,
                latest,
                history,
                comparison,
                environment_label,
            },
        );
    }

    // Feature views with health.
    let mut feature_views: BTreeMap<FeatureId, FeatureView> = BTreeMap::new();
    for f in features {
        let mut links_c = feature_commits.remove(&f.id).unwrap_or_default();
        links_c.sort_by_key(|l| std::cmp::Reverse(l.committed_at));
        links_c.truncate(200);
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
        let test_ids = feature_tests.remove(&f.id).unwrap_or_default();
        let visual_ids = feature_visuals.remove(&f.id).unwrap_or_default();
        let bench_ids = feature_benchmarks.remove(&f.id).unwrap_or_default();
        let required: Vec<EvidenceCategory> = cfg
            .health
            .required_for(f.importance.as_str())
            .iter()
            .filter_map(|r| EvidenceCategory::parse(r))
            .collect();
        let fin = crate::state::freshness::FreshnessInput {
            graphs: &graphs,
            impl_commit: impl_key.as_ref(),
            impl_at,
            max_age,
            now,
        };
        let tviews: Vec<&TestView> = test_ids.iter().filter_map(|t| tests.get(t)).collect();
        let vviews: Vec<&VisualEvidence> =
            visual_ids.iter().filter_map(|v| visuals.get(v)).map(|v| &v.latest).collect();
        let mut rollups = crate::state::rollup::rollups(&crate::state::rollup::RollupInput {
            tests: tviews.clone(),
            visuals: vviews,
            required: &required,
            freshness: &fin,
        });
        let bviews: Vec<&BenchmarkView> =
            bench_ids.iter().filter_map(|b| benchmarks.get(b)).collect();
        let (perf, mut extra_reasons) = crate::state::rollup::perf_rollup(
            &bviews,
            required.contains(&EvidenceCategory::Perf),
            &fin,
        );
        rollups.push(perf);
        for t in &tviews {
            if let Some((prev, latest, rel)) = t.duration_trend {
                extra_reasons.push(HealthReason {
                    reason: AttentionReason::TestDurationRegression,
                    severity: Severity::Low,
                    text: format!(
                        "{}: {} → {} (+{:.0}%)",
                        t.test.name,
                        crate::server::view::duration_ms(prev as f64),
                        crate::server::view::duration_ms(latest as f64),
                        rel * 100.0
                    ),
                    evidence: vec![EvidenceRef::new(
                        EvidenceKind::Test,
                        t.test.id.as_str(),
                        &t.test.name,
                    )],
                    confidence: Confidence::Medium,
                    subject: t.test.id.0.clone(),
                    commit: None,
                    category: Some(t.test.kind.category()),
                });
            }
        }
        // Claims vs evidence drift (informational): a component claimed shipped with no evidence.
        for c in &f.claims {
            if c.status == ClaimStatus::Shipped {
                let has = tviews.iter().any(|t| t.test.component.as_ref() == Some(&c.component));
                if !has && !tviews.is_empty() {
                    extra_reasons.push(HealthReason {
                        reason: AttentionReason::UnverifiedClaim,
                        severity: Severity::Informational,
                        text: format!(
                            "claims {} shipped, but no linked test runs on that component",
                            c.component
                        ),
                        evidence: vec![],
                        confidence: Confidence::Medium,
                        subject: c.component.0.clone(),
                        commit: None,
                        category: None,
                    });
                }
            }
        }
        let evidence = crate::state::health::Evidence { rollups, extra_reasons };
        let health = crate::state::health::evaluate(
            &f,
            &evidence,
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
                commits: links_c,
                adrs: adr_list,
                tests: test_ids,
                benchmarks: bench_ids,
                visuals: visual_ids,
            },
        );
    }

    // Suite views: silence.
    let suites_with_tests: HashSet<&SuiteId> = tests.values().map(|t| &t.test.suite).collect();
    let mut suite_views: Vec<Suite> = suites
        .iter()
        .filter(|su| {
            suites_with_tests.contains(&su.id)
                || su.last_run.as_ref().map(&run_is_current).unwrap_or(false)
        })
        .cloned()
        .collect();
    suite_views.sort_by(|a, b| a.id.cmp(&b.id));
    let mut suite_reasons: Vec<AttentionItem> = Vec::new();
    for su in &suite_views {
        let Some(expect) = su.expect_every else { continue };
        match su.last_seen {
            Some(seen) if now.duration_since(seen) > expect => suite_reasons
                .push(suite_silent_item(su, Some(now.duration_since(seen)), expect, now)),
            None => suite_reasons.push(suite_silent_item(su, None, expect, now)),
            _ => {}
        }
    }
    let unlinked_items: Vec<AttentionItem> = links
        .unlinked
        .iter()
        .map(|(suite, ids)| AttentionItem {
            id: crate::ids::AttentionId::derive("unlinked_tests", None, suite),
            reason: AttentionReason::UnlinkedTests,
            severity: Severity::Informational,
            status: AttentionStatus::Open,
            feature: None,
            feature_title: None,
            subject: suite.clone(),
            title: "Unlinked tests".into(),
            summary: format!(
                "{} test{} in {suite} map to no feature",
                ids.len(),
                if ids.len() == 1 { "" } else { "s" }
            ),
            why_it_matters: "Tests that map to no feature cannot support any feature's health."
                .into(),
            evidence: ids
                .iter()
                .take(5)
                .filter_map(|t| tests.get(t))
                .map(|t| EvidenceRef::new(EvidenceKind::Test, t.test.id.as_str(), &t.test.name))
                .collect(),
            confidence: Confidence::High,
            score: 0,
            first_seen: now,
            last_seen: now,
            resolved_at: None,
            commit: None,
            payload: serde_json::json!({ "count": ids.len() }),
        })
        .collect();

    // Benchmarks that belong to no feature still raise their regressions and breaches.
    let bench_items: Vec<AttentionItem> = benchmarks
        .values()
        .filter(|b| b.features.is_empty())
        .filter_map(|b| {
            let c = b.comparison.as_ref()?;
            use crate::state::benchmark_cmp::Classification;
            let reason = match c.classification {
                Classification::Regression => AttentionReason::BenchmarkRegression,
                Classification::BudgetBreach => AttentionReason::BenchmarkBudgetBreach,
                _ => return None,
            };
            Some(AttentionItem {
                id: crate::ids::AttentionId::derive(reason.as_str(), None, b.benchmark.id.as_str()),
                reason,
                severity: c.severity,
                status: AttentionStatus::Open,
                feature: None,
                feature_title: None,
                subject: b.benchmark.id.0.clone(),
                title: reason.label().to_string(),
                summary: format!("{}: {}", b.label(), c.note),
                why_it_matters: match reason {
                    AttentionReason::BenchmarkBudgetBreach => "A performance budget is a contract, and it is violated.".into(),
                    _ => "Performance moved in the wrong direction after a change.".into(),
                },
                evidence: vec![EvidenceRef::new(EvidenceKind::Benchmark, b.benchmark.id.as_str(), b.label())],
                confidence: c.confidence,
                score: 0,
                first_seen: now,
                last_seen: now,
                resolved_at: None,
                commit: c.origin.as_ref().and_then(|o| o.commit.as_ref()).map(|c| c.key.clone()),
                payload: serde_json::json!({ "classification": c.classification.as_str(), "determinism": c.determinism.as_str() }),
            })
        })
        .collect();

    // Attention: feature reasons + project-level items, merged with persisted first_seen.
    let mut extra_items = suite_reasons;
    extra_items.extend(unlinked_items);
    extra_items.extend(bench_items);
    let crate::state::attention::Collected { items: attention, resolved: resolved_attention } =
        crate::state::attention::collect(
            &feature_views,
            &warnings,
            &unresolved,
            extra_items,
            store,
            now,
        )?;

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
    counts.tests = tests.len();
    for t in tests.values() {
        match t.latest.as_ref().map(|r| r.status) {
            Some(TestStatus::Passed) => counts.tests_passed += 1,
            Some(TestStatus::Failed) => counts.tests_failed += 1,
            Some(TestStatus::Flaky) => counts.tests_flaky += 1,
            Some(TestStatus::Skipped) => counts.tests_skipped += 1,
            None => {}
        }
        if t.flaky && t.latest.as_ref().map(|r| r.status != TestStatus::Flaky).unwrap_or(false) {
            counts.tests_flaky += 1;
        }
    }
    counts.benchmarks = benchmarks.len();
    for b in benchmarks.values() {
        use crate::state::benchmark_cmp::Classification;
        match b.comparison.as_ref().map(|c| (c.classification, c.severity)) {
            Some((Classification::Regression, Severity::Informational)) => {
                counts.benchmark_reported += 1
            }
            Some((Classification::Regression, _)) => counts.benchmark_regressions += 1,
            Some((Classification::BudgetBreach, _)) => counts.benchmark_breaches += 1,
            Some((Classification::Incomparable, _)) => counts.benchmark_incomparable += 1,
            _ => {}
        }
    }
    counts.visuals = visuals.len();
    counts.visual_regressions =
        visuals.values().filter(|v| v.latest.status == VisualStatus::Regression).count();
    counts.visual_stale =
        visuals.values().filter(|v| v.latest.status == VisualStatus::Stale).count();
    let (overall, headline) = headline(&counts);

    // Persist derived rows; events are derived against what the store remembered before
    // this build overwrote it.
    store.batch_begin()?;
    let new_events = crate::state::events::produce(&crate::state::events::EventInput {
        store,
        features: &feature_views,
        adrs: &adr_views,
        benchmarks: &benchmarks,
        commits: &commits,
        attention: &attention,
        resolved: &resolved_attention,
        now,
    })?;
    store.replace_relationships(&relationships, &unresolved)?;
    for (id, v) in &feature_views {
        store.upsert_feature_health(id, &v.health)?;
    }
    for a in &attention {
        store.upsert_attention(a)?;
    }
    store.batch_commit()?;
    tracing::debug!("{} new events", new_events.len());

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
        suites: suite_views,
        tests,
        benchmarks,
        visuals,
        environments: environments_raw.into_iter().map(|e| (e.id.clone(), e)).collect(),
        artifacts: artifacts_raw.into_iter().map(|a| (a.id.clone(), a)).collect(),
        sources: summarize_sources(input.located, store),
    })
}

/// Feature id → its `paths:` globs (normalised), for ranking regression suspects.
fn feature_paths_by_id(store: &Store) -> crate::store::Result<HashMap<FeatureId, Vec<String>>> {
    Ok(store
        .features()?
        .into_iter()
        .map(|f| {
            let globs = f.paths.iter().map(|p| crate::link::ownership::normalise_glob(p)).collect();
            (f.id, globs)
        })
        .collect())
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

pub fn summarize_sources(located: &Located, store: &Store) -> Vec<SourceSummary> {
    let runs = store.runs().unwrap_or_default();
    let files = store.scan_files().unwrap_or_default();
    located
        .evidence_sources()
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let id = s.id.clone().unwrap_or_else(|| format!("auto:{}:{i}", s.format.as_str()));
            let my_runs: Vec<&Run> = runs.iter().filter(|r| r.source.as_str() == id).collect();
            SourceSummary {
                files_matched: files
                    .values()
                    .filter(|f| f.source_id.as_deref() == Some(id.as_str()))
                    .count(),
                runs: my_runs.len(),
                last_seen: my_runs.iter().map(|r| r.finished_at.value).max(),
                id,
                title: s.title.clone(),
                format: s.format.as_str().into(),
                kind: s.default_kind().into(),
                patterns: s.paths(),
                component: s.component.clone(),
                enabled: s.enabled,
                github: s
                    .github
                    .as_ref()
                    .map(|g| format!("{} · {} · {}", g.repo, g.workflow, g.artifact)),
                note: s.note.clone(),
            }
        })
        .collect()
}

fn suite_silent_item(
    su: &Suite,
    silent_for: Option<jiff::SignedDuration>,
    expect: jiff::SignedDuration,
    now: Timestamp,
) -> AttentionItem {
    let summary = match silent_for {
        Some(d) => format!(
            "{} silent for {} (expected every {})",
            su.id,
            crate::time::span_compact(d),
            crate::time::span_compact(expect)
        ),
        None => format!("{} has never published a run", su.id),
    };
    AttentionItem {
        id: crate::ids::AttentionId::derive("suite_silent", None, su.id.as_str()),
        reason: AttentionReason::SuiteSilent,
        severity: if silent_for.is_some() { Severity::Medium } else { Severity::Low },
        status: AttentionStatus::Open,
        feature: None,
        feature_title: None,
        subject: su.id.0.clone(),
        title: "Suite silent".into(),
        summary,
        why_it_matters:
            "Evidence that stops arriving goes stale quietly; silence must be louder than green."
                .into(),
        evidence: vec![EvidenceRef::new(EvidenceKind::Suite, su.id.as_str(), &su.id.0)],
        confidence: Confidence::High,
        score: 0,
        first_seen: now,
        last_seen: now,
        resolved_at: None,
        commit: None,
        payload: serde_json::json!({ "last_seen": su.last_seen.map(|t| t.to_string()) }),
    }
}

#[allow(dead_code)]
fn _ms(t: Timestamp) -> i64 {
    millis(t)
}
