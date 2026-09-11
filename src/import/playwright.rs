//! Playwright's JSON reporter (`--reporter=json`, or a merged blob report).

use super::batch::{BatchSpec, RawResult, RawVisual, assemble};
use super::provenance::Declared;
use super::{ImportContext, ImportInput, ImportOutput, ImportedEntity, Importer, ImporterKind};
use crate::config::EvidenceFormat;
use crate::ids::ArtifactId;
use crate::model::*;
use crate::paths::RelPath;
use crate::time::parse_timestamp;
use serde::Deserialize;
use std::path::Path;

pub struct PlaywrightImporter;

const NAME: &str = "playwright";

#[derive(Deserialize, Default)]
#[serde(default)]
struct Report {
    config: Config,
    suites: Vec<Suite>,
    stats: Stats,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Config {
    #[serde(rename = "rootDir")]
    root_dir: Option<String>,
    version: Option<String>,
    projects: Vec<Project>,
    metadata: serde_json::Value,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Project {
    id: Option<String>,
    name: Option<String>,
    #[serde(rename = "testDir")]
    test_dir: Option<String>,
    r#use: serde_json::Value,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Stats {
    #[serde(rename = "startTime")]
    start_time: Option<String>,
    duration: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Suite {
    title: String,
    file: Option<String>,
    tags: Vec<String>,
    specs: Vec<Spec>,
    suites: Vec<Suite>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Spec {
    title: String,
    tags: Vec<String>,
    tests: Vec<TestCase>,
    file: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TestCase {
    #[serde(rename = "projectName")]
    project_name: Option<String>,
    #[serde(rename = "projectId")]
    project_id: Option<String>,
    status: Option<String>,
    results: Vec<Attempt>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Attempt {
    status: Option<String>,
    duration: Option<f64>,
    #[serde(rename = "startTime")]
    start_time: Option<String>,
    error: Option<serde_json::Value>,
    errors: Vec<serde_json::Value>,
    attachments: Vec<Attachment>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Attachment {
    name: String,
    #[serde(rename = "contentType")]
    content_type: Option<String>,
    path: Option<String>,
}

/// Everything a suite visit needs; kept in one struct so recursion stays readable.
struct Walk<'a> {
    ctx: &'a ImportContext<'a>,
    input: &'a ImportInput,
    pkg_dir: RelPath,
    test_dir_hint: Option<String>,
    root_dir: Option<String>,
    projects: &'a [Project],
    results: Vec<RawResult>,
    latest_end: Option<jiff::Timestamp>,
}

impl Walk<'_> {
    fn project(&self, name: &Option<String>) -> Option<&Project> {
        self.projects.iter().find(|p| p.name == *name || p.id == *name)
    }

    fn viewport_of(&self, name: &Option<String>) -> Option<Viewport> {
        let vp = self.project(name)?.r#use.get("viewport")?;
        Some(Viewport {
            width: vp.get("width")?.as_u64()? as u32,
            height: vp.get("height")?.as_u64()? as u32,
        })
    }

    fn browser_of(&self, name: &Option<String>) -> Option<String> {
        let p = self.project(name)?;
        p.r#use.get("browserName").and_then(|b| b.as_str()).map(|s| s.to_string()).or_else(|| {
            p.r#use.get("defaultBrowserType").and_then(|b| b.as_str()).map(|s| s.to_string())
        })
    }

    fn visit(&mut self, suite: &Suite, file: &str, titles: &[String], tags: &[String]) {
        let file = suite.file.clone().unwrap_or_else(|| file.to_string());
        let mut tags: Vec<String> = tags.to_vec();
        tags.extend(suite.tags.iter().map(|t| t.trim_start_matches('@').to_string()));
        let mut titles: Vec<String> = titles.to_vec();
        // Root suites carry the file as their title; describe blocks carry real titles.
        if (suite.file.is_none() || suite.title != file) && !suite.title.is_empty() {
            titles.push(suite.title.clone());
        }
        for spec in &suite.specs {
            let spec_file = spec.file.clone().unwrap_or_else(|| file.clone());
            let mut spec_tags = tags.clone();
            spec_tags.extend(spec.tags.iter().map(|t| t.trim_start_matches('@').to_string()));
            for tok in spec.title.split_whitespace() {
                if let Some(t) = tok.strip_prefix('@') {
                    if t.contains(':') && !spec_tags.contains(&t.to_string()) {
                        spec_tags.push(t.to_string());
                    }
                }
            }
            let declared: Vec<String> = spec_tags
                .iter()
                .filter_map(|t| t.strip_prefix("feat:").map(|s| s.to_string()))
                .collect();
            let level =
                spec_tags.iter().find_map(|t| t.strip_prefix("level:").map(|s| s.to_string()));
            let full_title = if titles.is_empty() {
                spec.title.clone()
            } else {
                format!("{} › {}", titles.join(" › "), spec.title)
            };
            for t in &spec.tests {
                let project = t.project_name.clone().or(t.project_id.clone()).unwrap_or_default();
                let last = t.results.last();
                let attempts = t.results.len() as u32;
                let status = match (t.status.as_deref(), last.and_then(|a| a.status.as_deref())) {
                    (Some("skipped"), _) | (_, Some("skipped")) => TestStatus::Skipped,
                    (Some("flaky"), _) => TestStatus::Flaky,
                    (Some("expected"), _) => {
                        if attempts > 1 {
                            TestStatus::Flaky
                        } else {
                            TestStatus::Passed
                        }
                    }
                    (Some("unexpected"), _) => TestStatus::Failed,
                    (_, Some("passed")) => TestStatus::Passed,
                    (_, Some("failed")) | (_, Some("timedOut")) | (_, Some("interrupted")) => {
                        TestStatus::Failed
                    }
                    _ => TestStatus::Skipped,
                };
                let start = last.and_then(|a| a.start_time.as_deref()).and_then(parse_timestamp);
                let dur_ms = last.and_then(|a| a.duration).map(|d| d.round() as u64);
                if let (Some(s), Some(d)) = (start, dur_ms) {
                    let end =
                        s.checked_add(jiff::SignedDuration::from_millis(d as i64)).unwrap_or(s);
                    if self.latest_end.map(|l| end > l).unwrap_or(true) {
                        self.latest_end = Some(end);
                    }
                }
                // The last attempt that recorded an error (a final retry may omit it).
                let error =
                    t.results.iter().rev().find_map(|a| a.error.as_ref().or(a.errors.first()));
                let message =
                    error.and_then(|e| e.get("message")).and_then(|m| m.as_str()).map(first_line);
                let details = error
                    .and_then(|e| e.get("stack").or(e.get("message")))
                    .and_then(|m| m.as_str())
                    .map(strip_ansi);
                let with_attachments =
                    t.results.iter().rev().find(|a| !a.attachments.is_empty()).or(last);
                let (artifacts, visuals) = collect_attachments(
                    with_attachments.map(|a| a.attachments.as_slice()).unwrap_or(&[]),
                    self.ctx,
                    self.input,
                    &self.root_dir,
                    status,
                );
                let source_path =
                    resolve_spec_path(self.ctx, &self.pkg_dir, &self.test_dir_hint, &spec_file);
                let key = format!("{spec_file}::{full_title}::{project}");
                self.results.push(RawResult {
                    key,
                    name: if project.is_empty() {
                        full_title.clone()
                    } else {
                        format!("{full_title} [{project}]")
                    },
                    classname: Some(spec_file.clone()),
                    status: Some(status),
                    duration_ms: dur_ms,
                    retries: attempts.saturating_sub(1),
                    timestamp: start,
                    message,
                    details,
                    tags: spec_tags.clone(),
                    declared_feature_ids: declared.clone(),
                    source_path,
                    source_path_origin: Some(crate::time::Origin::Inferred),
                    kind: None,
                    component: None,
                    level: level.clone(),
                    browser: self
                        .browser_of(&t.project_name)
                        .or_else(|| if project.is_empty() { None } else { Some(project.clone()) }),
                    viewport: self.viewport_of(&t.project_name),
                    artifacts,
                    visuals,
                });
            }
        }
        for child in &suite.suites {
            self.visit(child, &file, &titles, &tags);
        }
    }
}

impl Importer for PlaywrightImporter {
    fn kind(&self) -> ImporterKind {
        ImporterKind::Evidence(EvidenceFormat::Playwright)
    }
    fn version(&self) -> u32 {
        1
    }
    fn import(&self, ctx: &ImportContext, input: &ImportInput) -> ImportOutput {
        let mut out = ImportOutput::default();
        let report: Report = match serde_json::from_slice(&input.bytes) {
            Ok(r) => r,
            Err(e) => {
                out.fatal(
                    &input.rel,
                    NAME,
                    format!("not a Playwright JSON report: {e}"),
                    Some(format!("line {}", e.line())),
                );
                return out;
            }
        };
        let mut walk = Walk {
            ctx,
            input,
            pkg_dir: package_dir_of(&input.rel, &ctx.located.root),
            test_dir_hint: report.config.projects.first().and_then(|p| p.test_dir.clone()),
            root_dir: report.config.root_dir.clone(),
            projects: &report.config.projects,
            results: Vec::new(),
            latest_end: None,
        };
        for s in &report.suites {
            walk.visit(s, s.file.as_deref().unwrap_or(""), &[], &[]);
        }
        let finished = walk.latest_end.or_else(|| {
            let start = report.stats.start_time.as_deref().and_then(parse_timestamp)?;
            Some(
                start
                    .checked_add(jiff::SignedDuration::from_millis(
                        report.stats.duration.unwrap_or(0.0) as i64,
                    ))
                    .unwrap_or(start),
            )
        });
        let results = std::mem::take(&mut walk.results);
        let meta = &report.config.metadata;
        let get = |keys: &[&str]| {
            keys.iter().find_map(|k| meta.get(*k)).and_then(|v| {
                v.as_str().map(|s| s.to_string()).or_else(|| v.as_u64().map(|n| n.to_string()))
            })
        };
        let decl = Declared {
            commit: get(&["git.commit", "commit", "gitCommit"]),
            branch: get(&["git.branch", "branch"]),
            timestamp: finished,
            tool: Some(ToolInfo {
                name: "playwright".into(),
                version: report.config.version.clone(),
            }),
            run_id: get(&["ci.run_id", "runId", "run_id"]),
            run_url: get(&["ci.run_url", "runUrl", "run_url"]),
            dirty: None,
            repo: None,
        };
        let suite_name =
            ctx.source.and_then(|s| s.suite.clone()).unwrap_or_else(|| "playwright".into());
        let default_kind =
            ctx.source.map(|s| TestKind::parse(s.default_kind())).unwrap_or(TestKind::E2e);
        let (batch, notes) = assemble(
            ctx,
            BatchSpec {
                format: EvidenceFormat::Playwright,
                version: self.version(),
                suite_name,
                default_kind,
                declared: decl,
                run_identity: None,
                results,
                input,
                source_override: None,
            },
        );
        for n in notes {
            out.warn(&input.rel, NAME, n, None);
        }
        out.entities.push(ImportedEntity::TestBatch(batch));
        out
    }
}

fn first_line(s: &str) -> String {
    strip_ansi(s.lines().next().unwrap_or("").trim())
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

pub(crate) fn package_dir_of(rel: &RelPath, root: &Path) -> RelPath {
    let mut dir = rel.parent();
    loop {
        if dir.join("package.json").to_abs(root).exists()
            || dir.join("playwright.config.ts").to_abs(root).exists()
        {
            return dir;
        }
        if dir.is_root() {
            return rel.parent().parent();
        }
        dir = dir.parent();
    }
}

fn resolve_spec_path(
    ctx: &ImportContext,
    pkg_dir: &RelPath,
    test_dir_hint: &Option<String>,
    spec_file: &str,
) -> Option<RelPath> {
    let root = &ctx.located.root;
    let mut candidates: Vec<RelPath> = Vec::new();
    if let Some(td) = test_dir_hint {
        let td = td.trim_start_matches("./");
        if !Path::new(td).is_absolute() {
            candidates.push(pkg_dir.join(td).join(spec_file));
        }
    }
    candidates.push(pkg_dir.join("tests").join(spec_file));
    candidates.push(pkg_dir.join("test").join(spec_file));
    candidates.push(pkg_dir.join("e2e").join(spec_file));
    candidates.push(pkg_dir.join(spec_file));
    for c in &candidates {
        if c.to_abs(root).exists() {
            return Some(c.clone());
        }
    }
    candidates.into_iter().find(|c| c.as_str().contains("/tests/"))
}

fn collect_attachments(
    atts: &[Attachment],
    ctx: &ImportContext,
    input: &ImportInput,
    root_dir: &Option<String>,
    status: TestStatus,
) -> (Vec<Artifact>, Vec<RawVisual>) {
    let mut artifacts = Vec::new();
    let mut triples: std::collections::BTreeMap<String, Triple> = Default::default();
    for a in atts {
        let Some(path) = &a.path else { continue };
        let art_path = rebase(path, ctx, input, root_dir);
        let exists = match &art_path {
            ArtifactPath::Project(p) => p.to_abs(&ctx.located.root).exists(),
            ArtifactPath::External(p) => p.exists(),
            _ => false,
        };
        let key = match &art_path {
            ArtifactPath::Project(p) => p.as_str().to_string(),
            ArtifactPath::External(p) => p.display().to_string(),
            ArtifactPath::EvidenceCache(p) => p.as_str().to_string(),
            ArtifactPath::Url(u) => u.clone(),
        };
        let ct = a.content_type.clone().unwrap_or_default();
        let lower = a.name.to_ascii_lowercase();
        let (role, base) = if let Some(b) =
            lower.strip_suffix("-expected.png").or_else(|| lower.strip_suffix("-expected.jpg"))
        {
            (ArtifactRole::Expected, Some(b.to_string()))
        } else if let Some(b) =
            lower.strip_suffix("-actual.png").or_else(|| lower.strip_suffix("-actual.jpg"))
        {
            (ArtifactRole::Actual, Some(b.to_string()))
        } else if let Some(b) =
            lower.strip_suffix("-diff.png").or_else(|| lower.strip_suffix("-diff.jpg"))
        {
            (ArtifactRole::Diff, Some(b.to_string()))
        } else {
            (ArtifactRole::Plain, None)
        };
        let kind = if ct.starts_with("image/") {
            ArtifactKind::Screenshot
        } else if ct.starts_with("video/") {
            ArtifactKind::Video
        } else if a.name == "trace" || key.ends_with("trace.zip") {
            ArtifactKind::Trace
        } else {
            ArtifactKind::from_content_type(&ct)
        };
        let art = Artifact {
            id: ArtifactId::derive(&key),
            kind,
            role,
            name: a.name.clone(),
            path: art_path,
            content_type: a.content_type.clone(),
            size: None,
            exists,
        };
        if let Some(b) = base {
            let e = triples.entry(b).or_default();
            match role {
                ArtifactRole::Expected => e.0 = Some(art),
                ArtifactRole::Actual => e.1 = Some(art),
                _ => e.2 = Some(art),
            }
        } else {
            artifacts.push(art);
        }
    }
    let mut visuals = Vec::new();
    for (name, (expected, actual, diff)) in triples {
        let vstatus = if diff.is_some() || status == TestStatus::Failed {
            VisualStatus::Regression
        } else if expected.is_none() {
            VisualStatus::Missing
        } else {
            VisualStatus::Current
        };
        visuals.push(RawVisual {
            name,
            page: None,
            baseline: expected,
            actual,
            diff,
            metrics: DiffMetrics::default(),
            status: vstatus,
        });
    }
    (artifacts, visuals)
}

/// (expected, actual, diff) for one snapshot name.
type Triple = (Option<Artifact>, Option<Artifact>, Option<Artifact>);

/// Map an attachment path (often absolute, often from another machine) into the project.
fn rebase(
    path: &str,
    ctx: &ImportContext,
    input: &ImportInput,
    root_dir: &Option<String>,
) -> ArtifactPath {
    let root = &ctx.located.root;
    let p = Path::new(path);
    if !p.is_absolute() {
        return ArtifactPath::Project(input.rel.parent().join(path));
    }
    if let Some(rel) = crate::paths::relativize(root, p) {
        return ArtifactPath::Project(rel);
    }
    if let Some(rd) = root_dir {
        if let Ok(tail) = p.strip_prefix(rd) {
            let pkg = package_dir_of(&input.rel, root);
            return ArtifactPath::Project(pkg.join(&tail.to_string_lossy()));
        }
    }
    if let Some(i) = path.find("test-results/") {
        let tail = &path[i + "test-results/".len()..];
        let base = if input.rel.parent().file_name() == "test-results" {
            input.rel.parent()
        } else {
            input.rel.parent().join("test-results")
        };
        return ArtifactPath::Project(base.join(tail));
    }
    ArtifactPath::External(p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::super::feature_md::tests::{input, located, project};
    use super::*;
    use std::collections::HashSet;

    const REPORT: &str = r#"{
      "config": { "rootDir": "/home/runner/work/editor/editor/apps/canvas", "version": "1.55.0", "projects": [{"id":"chromium","name":"chromium","testDir":"/home/runner/work/editor/editor/apps/canvas/tests","use":{"browserName":"chromium","viewport":{"width":1440,"height":900}}}], "metadata": {"git.commit": "abc1234"} },
      "suites": [ { "title": "layers-panel.spec.ts", "file": "layers-panel.spec.ts", "specs": [
          { "title": "AC-LAYERS-3 drag reorder @feat:layers.ops @level:gesture", "tags": ["feat:layers.ops", "level:gesture"], "tests": [ { "projectName": "chromium", "status": "unexpected", "results": [
              { "status": "failed", "duration": 8120, "retry": 0, "startTime": "2026-08-22T14:18:00.000Z", "error": {"message": "\u001b[31mexpect(locator).toHaveText\u001b[0m failed"},
                "attachments": [ {"name": "screenshot", "contentType": "image/png", "path": "/home/runner/work/editor/editor/apps/canvas/test-results/layers-drag/test-failed-1.png"}, {"name":"trace","contentType":"application/zip","path":"/home/runner/work/editor/editor/apps/canvas/test-results/layers-drag/trace.zip"} ] },
              { "status": "failed", "duration": 8000, "retry": 1, "startTime": "2026-08-22T14:18:10.000Z", "attachments": [] } ] } ] } ],
          "suites": [ { "title": "visuals", "tags": ["@feat:editor-shell.panels.layers"], "specs": [
             { "title": "matches baseline", "tags": [], "tests": [ { "projectName": "chromium", "status": "expected", "results": [ { "status": "passed", "duration": 300, "retry": 0, "startTime": "2026-08-22T14:19:00.000Z", "attachments": [ {"name":"panel-expected.png","contentType":"image/png","path":"/home/runner/work/editor/editor/apps/canvas/test-results/v/panel-expected.png"}, {"name":"panel-actual.png","contentType":"image/png","path":"/home/runner/work/editor/editor/apps/canvas/test-results/v/panel-actual.png"} ] } ] } ] },
             { "title": "skipped one", "tests": [ { "projectName": "chromium", "status": "skipped", "results": [] } ] } ] } ] } ],
      "stats": { "startTime": "2026-08-22T14:17:00.000Z", "duration": 180000, "expected": 1, "unexpected": 1, "flaky": 0, "skipped": 1 } }"#;

    #[test]
    fn parses_nested_suites_tags_attempts_and_attachments() {
        let loc = located();
        let proj = project();
        let repos = super::super::RepoIndex::default();
        let ids: HashSet<crate::ids::FeatureId> = HashSet::new();
        let ctx = ImportContext {
            located: &loc,
            project: &proj,
            repos: &repos,
            feature_ids: &ids,
            source: None,
            source_id: None,
            now: crate::time::now(),
            cargo: &Default::default(),
            sidecars: &Default::default(),
            commits: &Default::default(),
        };
        let out = PlaywrightImporter
            .import(&ctx, &input("editor/apps/canvas/test-results/results.json", REPORT));
        let ImportedEntity::TestBatch(b) = &out.entities[0] else { panic!("{:?}", out.warnings) };
        assert_eq!(b.tests.len(), 3);
        let t0 = &b.tests[0];
        assert_eq!(
            t0.key,
            "layers-panel.spec.ts::AC-LAYERS-3 drag reorder @feat:layers.ops @level:gesture::chromium"
        );
        assert_eq!(t0.declared_feature_ids, vec!["layers.ops"]);
        assert_eq!(t0.level.as_deref(), Some("gesture"));
        assert_eq!(b.test_runs[0].status, TestStatus::Failed);
        assert_eq!(b.test_runs[0].retries, 1);
        assert_eq!(b.test_runs[0].message.as_deref(), Some("expect(locator).toHaveText failed"));
        assert_eq!(b.test_runs[0].browser.as_deref(), Some("chromium"));
        assert_eq!(b.test_runs[0].viewport, Some(Viewport { width: 1440, height: 900 }));
        assert_eq!(b.test_runs[0].artifacts.len(), 2);
        let shot = b
            .artifacts
            .iter()
            .find(|a| a.kind == ArtifactKind::Screenshot && a.role == ArtifactRole::Plain)
            .unwrap();
        assert_eq!(
            shot.path,
            ArtifactPath::Project(RelPath::new(
                "editor/apps/canvas/test-results/layers-drag/test-failed-1.png"
            ))
        );
        let t1 = &b.tests[1];
        assert_eq!(t1.name, "visuals › matches baseline [chromium]");
        assert_eq!(t1.declared_feature_ids, vec!["editor-shell.panels.layers"]);
        assert_eq!(b.visuals.len(), 1);
        assert_eq!(b.visuals[0].status, VisualStatus::Current);
        assert!(
            b.visuals[0].baseline.is_some()
                && b.visuals[0].actual.is_some()
                && b.visuals[0].diff.is_none()
        );
        assert_eq!(b.test_runs[2].status, TestStatus::Skipped);
        assert_eq!(b.run.provenance.commit.as_ref().unwrap().key.sha.as_str(), "abc1234");
        assert_eq!(b.run.finished_at.value.to_string(), "2026-08-22T14:19:00.3Z");
        assert_eq!(b.run.provenance.tool.as_ref().unwrap().version.as_deref(), Some("1.55.0"));
    }
}
