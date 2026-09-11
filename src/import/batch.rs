//! Shared assembly of a `TestBatch` from parsed raw results: suites, runs, ids, counts, the
//! silence and validity rules. Every test importer funnels through here so the semantics are
//! identical regardless of format.

use super::provenance::{Built, Declared};
use super::{ImportContext, ImportInput, TestBatch};
use crate::config::EvidenceFormat;
use crate::ids::{ComponentId, RepoId, RunId, SuiteId, TestId, TestRunId};
use crate::model::*;
use crate::paths::RelPath;
use crate::time::{Origin, Stamped};
use jiff::Timestamp;

#[derive(Debug, Clone, Default)]
pub struct RawResult {
    pub key: String,
    pub name: String,
    pub classname: Option<String>,
    pub status: Option<TestStatus>,
    pub duration_ms: Option<u64>,
    pub retries: u32,
    pub timestamp: Option<Timestamp>,
    pub message: Option<String>,
    pub details: Option<String>,
    pub tags: Vec<String>,
    pub declared_feature_ids: Vec<String>,
    pub source_path: Option<RelPath>,
    pub source_path_origin: Option<Origin>,
    pub kind: Option<TestKind>,
    pub component: Option<ComponentId>,
    pub level: Option<String>,
    pub browser: Option<String>,
    pub viewport: Option<Viewport>,
    pub artifacts: Vec<Artifact>,
    pub visuals: Vec<RawVisual>,
}

#[derive(Debug, Clone)]
pub struct RawVisual {
    pub name: String,
    pub page: Option<u32>,
    pub baseline: Option<Artifact>,
    pub actual: Option<Artifact>,
    pub diff: Option<Artifact>,
    pub metrics: DiffMetrics,
    pub status: VisualStatus,
}

pub struct BatchSpec<'a> {
    pub format: EvidenceFormat,
    pub version: u32,
    pub suite_name: String,
    pub default_kind: TestKind,
    pub declared: Declared,
    pub run_identity: Option<String>,
    pub results: Vec<RawResult>,
    pub input: &'a ImportInput,
    /// When the document names its own source (paged-results), the batch belongs to that
    /// configured source rather than the one whose glob matched the file.
    pub source_override: Option<(crate::ids::SourceId, Option<&'a crate::config::EvidenceSource>)>,
}

/// Build the batch. `notes` carries provenance remarks for warnings.
pub fn assemble(ctx: &ImportContext, spec: BatchSpec) -> (TestBatch, Vec<String>) {
    let (source_id, source_cfg) = match &spec.source_override {
        Some((id, cfg)) => (id.clone(), cfg.or(ctx.source)),
        None => (
            ctx.source_id.clone().unwrap_or_else(|| {
                crate::ids::SourceId::new(format!("auto:{}", spec.format.as_str()))
            }),
            ctx.source,
        ),
    };
    let suite_id =
        SuiteId::new(if spec.suite_name.is_empty() || spec.suite_name == source_id.as_str() {
            source_id.as_str().to_string()
        } else {
            format!("{}#{}", source_id, spec.suite_name)
        });
    let mut declared = spec.declared;
    if declared.repo.is_none() {
        declared.repo = source_cfg.and_then(|s| s.repo.clone()).map(RepoId::new);
    }
    let Built { mut provenance, timestamp, mut notes } = super::provenance::build(
        ctx,
        spec.format,
        spec.version,
        &spec.input.rel,
        &spec.input.abs,
        spec.input.content_hash,
        spec.input.mtime,
        declared,
    );
    provenance.source = source_id.clone();
    let identity = spec
        .run_identity
        .clone()
        .or_else(|| provenance.run_id.clone())
        .unwrap_or_else(|| spec.input.content_hash.hex());
    let run_id = RunId::derive(&source_id, &identity);

    let component = source_cfg.and_then(|s| s.component.clone()).map(ComponentId::new);
    let repo: Option<RepoId> = provenance.commit.as_ref().map(|c| c.key.repo.clone());
    let mut counts = RunCounts::default();
    let mut tests = Vec::new();
    let mut test_runs = Vec::new();
    let mut artifacts = Vec::new();
    let mut visuals = Vec::new();
    let mut latest_ts: Option<Timestamp> = None;
    let results_declare_times = spec.results.iter().any(|r| r.timestamp.is_some());
    for r in spec.results {
        let status = r.status.unwrap_or(TestStatus::Skipped);
        counts.add(status);
        let test_id = TestId::derive(&suite_id, &r.key);
        let ts = r.timestamp.map(|t| Stamped::new(t, Origin::Declared)).unwrap_or(timestamp);
        if latest_ts.map(|l| ts.value > l).unwrap_or(true) {
            latest_ts = Some(ts.value);
        }
        let mut test = Test {
            id: test_id.clone(),
            suite: suite_id.clone(),
            key: r.key.clone(),
            name: r.name.clone(),
            classname: r.classname.clone(),
            kind: r.kind.unwrap_or(spec.default_kind),
            source_path: r.source_path.clone(),
            source_path_origin: r.source_path_origin,
            line: None,
            tags: r.tags.clone(),
            component: r.component.clone().or_else(|| component.clone()),
            declared_feature_ids: r.declared_feature_ids.clone(),
            level: r.level.clone(),
            first_seen: ts.value,
            last_seen: ts.value,
        };
        // Feature ids from tags / names via the linker's regexes, recorded now so the store has them.
        for found in ctx_find_refs(&r) {
            if !test.declared_feature_ids.contains(&found) {
                test.declared_feature_ids.push(found);
            }
        }
        let tr_id = TestRunId::derive(&run_id, &test_id);
        let art_ids: Vec<crate::ids::ArtifactId> =
            r.artifacts.iter().map(|a| a.id.clone()).collect();
        artifacts.extend(r.artifacts.iter().cloned());
        for v in &r.visuals {
            let check = crate::ids::VisualCheckId::derive(&suite_id, &r.key, &v.name);
            let mut arts = vec![];
            for a in [&v.baseline, &v.actual, &v.diff].into_iter().flatten() {
                arts.push(a.id.clone());
                artifacts.push(a.clone());
            }
            visuals.push(VisualEvidence {
                id: crate::ids::VisualEvidenceId::derive(&check, &run_id),
                check,
                run: run_id.clone(),
                name: v.name.clone(),
                test: Some(test_id.clone()),
                test_run: Some(tr_id.clone()),
                page: v.page,
                browser: r.browser.clone(),
                viewport: r.viewport,
                baseline: v.baseline.as_ref().map(|a| a.id.clone()),
                actual: v.actual.as_ref().map(|a| a.id.clone()),
                diff: v.diff.as_ref().map(|a| a.id.clone()),
                metrics: v.metrics.clone(),
                status: v.status,
                commit: provenance.commit.clone(),
                timestamp: ts,
                declared_feature_ids: test.declared_feature_ids.clone(),
            });
        }
        test_runs.push(TestRun {
            id: tr_id,
            run: run_id.clone(),
            test: test_id,
            status,
            duration_ms: r.duration_ms,
            retries: r.retries,
            timestamp: ts,
            message: r.message.clone(),
            details: r.details.clone().map(|d| truncate(&d, 8 * 1024)),
            artifacts: art_ids,
            browser: r.browser.clone(),
            viewport: r.viewport,
            commit: provenance.commit.clone(),
        });
        tests.push(test);
    }

    // Silence / validity (paged lessons): no results, all skipped, or a uniform mass failure.
    let total = counts.total();
    let mut silent = false;
    let mut silent_reason = None;
    if total == 0 {
        silent = true;
        silent_reason = Some("no results in the document".into());
    } else if counts.skipped == total {
        silent = true;
        silent_reason = Some("every result is skipped".into());
    } else if let Some(v) = source_cfg.map(|s| &s.validity) {
        if v.treat_all_failed_as_invalid
            && counts.failed == total
            && total >= v.all_failed_min_tests
        {
            silent = true;
            silent_reason = Some(format!(
                "{total} of {total} failed and none passed — the run probably had no inputs; treated as invalid, not red"
            ));
        }
    }
    if silent {
        if let Some(r) = &silent_reason {
            notes.push(r.clone());
        }
    }
    let finished_at = match timestamp.origin {
        Origin::Declared => timestamp,
        _ => latest_ts
            .filter(|_| results_declare_times)
            .map(|t| Stamped::new(t, Origin::Declared))
            .unwrap_or(timestamp),
    };
    let suite = Suite {
        id: suite_id,
        source: source_id.clone(),
        name: spec.suite_name.clone(),
        default_kind: spec.default_kind,
        component,
        repo,
        expect_every: source_cfg.and_then(|s| s.expect_every(&ctx.located.config.health)),
        last_seen: Some(finished_at.value),
        last_run: Some(run_id.clone()),
        run_count: 1,
    };
    let run = Run {
        id: run_id,
        source: source_id,
        suite: Some(suite.id.clone()),
        kind: if visuals.len() == tests.len() && !tests.is_empty() {
            RunKind::Visual
        } else {
            RunKind::Test
        },
        provenance,
        finished_at,
        started_at: None,
        counts,
        silent,
        silent_reason,
    };
    (TestBatch { suite, run, tests, test_runs, artifacts, visuals }, notes)
}

/// Feature references a test carries in its own name/tags (`@feat:`, `__feat__`, `[ID]`).
fn ctx_find_refs(r: &RawResult) -> Vec<String> {
    let mut out = Vec::new();
    let mut text = r.name.clone();
    for t in &r.tags {
        text.push(' ');
        text.push_str(t);
    }
    if let Some(c) = &r.classname {
        text.push(' ');
        text.push_str(c);
    }
    for c in crate::link::refs::raw_refs(&text) {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}
