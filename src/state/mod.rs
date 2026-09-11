//! The derived engineering state: what the server renders and the CLI prints. Rebuilt whole
//! from the store after every scan and swapped atomically.

pub mod attention;
pub mod build;
pub mod health;

use crate::ids::*;
use crate::model::*;
use arc_swap::ArcSwap;
use jiff::Timestamp;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize)]
pub struct FeatureView {
    pub feature: Feature,
    pub health: FeatureHealth,
    /// Newest implementation commit touching this feature (any repo).
    pub impl_commit: Option<CommitKey>,
    pub impl_committed_at: Option<Timestamp>,
    pub commits: Vec<CommitLink>,
    pub adrs: Vec<AdrId>,
    pub tests: Vec<TestId>,
    pub benchmarks: Vec<BenchmarkId>,
    pub visuals: Vec<VisualCheckId>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommitLink {
    pub key: CommitKey,
    pub origin: RelationOrigin,
    pub committed_at: Timestamp,
}

#[derive(Clone, Debug, Serialize)]
pub struct AdrView {
    pub adr: Adr,
    pub features: Vec<FeatureId>,
    pub citing_commits: Vec<CommitKey>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommitView {
    pub commit: Commit,
    pub features: Vec<(FeatureId, RelationOrigin)>,
    pub adr_refs: Vec<AdrId>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct StateCounts {
    pub features: usize,
    pub healthy: usize,
    pub attention: usize,
    pub broken: usize,
    pub unknown: usize,
    pub adrs: usize,
    pub adrs_accepted: usize,
    pub adrs_proposed: usize,
    pub commits: usize,
    pub tests: usize,
    pub tests_passed: usize,
    pub tests_failed: usize,
    pub tests_flaky: usize,
    pub tests_skipped: usize,
    pub benchmarks: usize,
    pub benchmark_regressions: usize,
    pub benchmark_breaches: usize,
    pub visuals: usize,
    pub visual_regressions: usize,
    pub visual_stale: usize,
    pub attention_items: usize,
    pub warnings: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectState {
    pub project: Project,
    pub generation: u64,
    pub generated_at: Timestamp,
    pub last_scan_at: Option<Timestamp>,
    pub last_scan_duration_ms: Option<u64>,
    pub counts: StateCounts,
    /// Overall one-liner ("Healthy with 3 items needing attention").
    pub headline: String,
    pub overall: HealthState,
    pub features: BTreeMap<FeatureId, FeatureView>,
    pub adrs: BTreeMap<AdrId, AdrView>,
    pub commits: HashMap<CommitKey, CommitView>,
    /// Newest first across repos, capped.
    pub recent_commits: Vec<CommitKey>,
    pub tags: Vec<(RepoId, String, Sha)>,
    pub attention: Vec<AttentionItem>,
    pub events: Vec<EngineeringEvent>,
    pub unresolved: Vec<UnresolvedRef>,
    pub warnings: Vec<ImportWarning>,
    pub suites: Vec<Suite>,
    pub tests: BTreeMap<TestId, TestView>,
    pub benchmarks: BTreeMap<BenchmarkId, BenchmarkView>,
    pub visuals: BTreeMap<VisualCheckId, VisualView>,
    pub environments: HashMap<EnvironmentId, EnvironmentFingerprint>,
    pub artifacts: HashMap<ArtifactId, Artifact>,
    pub sources: Vec<SourceSummary>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TestView {
    pub test: Test,
    pub features: Vec<(FeatureId, RelationOrigin)>,
    pub latest: Option<TestRun>,
    /// Newest first, capped.
    pub history: Vec<TestRun>,
    pub flaky: bool,
    pub flaky_text: Option<String>,
    pub duration_trend: Option<(u64, u64, f64)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkView {
    pub benchmark: Benchmark,
    pub features: Vec<(FeatureId, RelationOrigin)>,
    pub latest: Option<BenchmarkRun>,
    pub history: Vec<BenchmarkRun>,
    pub comparison: Option<crate::state::build::BenchmarkComparison>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VisualView {
    pub latest: VisualEvidence,
    pub previous: Option<VisualEvidence>,
    pub features: Vec<(FeatureId, RelationOrigin)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceSummary {
    pub id: String,
    pub title: Option<String>,
    pub format: String,
    pub kind: String,
    pub patterns: Vec<String>,
    pub component: Option<String>,
    pub enabled: bool,
    pub files_matched: usize,
    pub runs: usize,
    pub last_seen: Option<Timestamp>,
    pub github: Option<String>,
    pub note: Option<String>,
}

impl ProjectState {
    pub fn empty(project: Project) -> ProjectState {
        ProjectState {
            project,
            generation: 0,
            generated_at: crate::time::now(),
            last_scan_at: None,
            last_scan_duration_ms: None,
            counts: StateCounts::default(),
            headline: "Not scanned yet".into(),
            overall: HealthState::Unknown,
            features: BTreeMap::new(),
            adrs: BTreeMap::new(),
            commits: HashMap::new(),
            recent_commits: vec![],
            tags: vec![],
            attention: vec![],
            events: vec![],
            unresolved: vec![],
            warnings: vec![],
            suites: vec![],
            tests: BTreeMap::new(),
            benchmarks: BTreeMap::new(),
            visuals: BTreeMap::new(),
            environments: HashMap::new(),
            artifacts: HashMap::new(),
            sources: vec![],
        }
    }

    pub fn feature(&self, id: &str) -> Option<&FeatureView> {
        self.features.get(&FeatureId::new(id))
    }

    pub fn commit(&self, repo: &RepoId, sha: &Sha) -> Option<&CommitView> {
        self.commits.get(&CommitKey { repo: repo.clone(), sha: sha.clone() })
    }

    /// Cross-repo prefix lookup.
    pub fn find_commits(&self, prefix: &str) -> Vec<&CommitView> {
        let p = prefix.to_ascii_lowercase();
        self.commits.values().filter(|c| c.commit.key.sha.as_str().starts_with(&p)).collect()
    }

    pub fn features_sorted(&self) -> Vec<&FeatureView> {
        let mut v: Vec<&FeatureView> = self.features.values().collect();
        v.sort_by(|a, b| {
            b.health
                .rank_score
                .cmp(&a.health.rank_score)
                .then_with(|| a.feature.id.as_str().cmp(b.feature.id.as_str()))
        });
        v
    }
}

/// Lock-free swappable snapshot shared by the server and the scanner.
#[derive(Clone)]
pub struct StateHandle(Arc<ArcSwap<ProjectState>>);

impl StateHandle {
    pub fn new(state: ProjectState) -> Self {
        StateHandle(Arc::new(ArcSwap::from_pointee(state)))
    }
    pub fn load(&self) -> Arc<ProjectState> {
        self.0.load_full()
    }
    pub fn store(&self, state: ProjectState) {
        self.0.store(Arc::new(state));
    }
}
