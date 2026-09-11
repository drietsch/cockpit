//! `cockpit.results.v1` and its ancestor `paged-results.json` (contract 1): one parser.

use super::batch::{BatchSpec, RawResult, assemble};
use super::provenance::Declared;
use super::{ImportContext, ImportInput, ImportOutput, ImportedEntity, Importer, ImporterKind};
use crate::config::EvidenceFormat;
use crate::ids::{ArtifactId, ComponentId};
use crate::model::*;
use crate::paths::RelPath;
use crate::time::parse_timestamp;
use serde::Deserialize;

pub struct ResultsJsonImporter {
    pub format: EvidenceFormat,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Doc {
    schema: Option<String>,
    contract: Option<u32>,
    source: Option<String>,
    suite: Option<String>,
    kind: Option<String>,
    component: Option<String>,
    commit: Option<String>,
    branch: Option<String>,
    run_id: Option<serde_json::Value>,
    run_url: Option<String>,
    finished_at: Option<String>,
    started_at: Option<String>,
    tool: Option<ToolDoc>,
    provenance: Option<ProvDoc>,
    features: Vec<String>,
    results: Vec<ResultDoc>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ToolDoc {
    name: Option<String>,
    version: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ProvDoc {
    commit: Option<String>,
    branch: Option<String>,
    dirty: Option<bool>,
    generated_at: Option<String>,
    tool: Option<ToolDoc>,
    run: Option<RunDoc>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RunDoc {
    id: Option<serde_json::Value>,
    url: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ResultDoc {
    test_id: String,
    title: Option<String>,
    file: Option<String>,
    features: Vec<String>,
    status: Option<String>,
    duration_ms: Option<f64>,
    retries: Option<u32>,
    #[serde(alias = "stage")]
    component: Option<String>,
    #[serde(alias = "coverage_level")]
    level: Option<String>,
    document: Option<String>,
    note: Option<String>,
    error: Option<ErrorDoc>,
    artifacts: Vec<ArtifactDoc>,
    trace_url: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ErrorDoc {
    message: Option<String>,
    stack: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ArtifactDoc {
    #[serde(rename = "type")]
    kind: Option<String>,
    path: Option<String>,
    url: Option<String>,
    mime: Option<String>,
    label: Option<String>,
}

fn val_string(v: &Option<serde_json::Value>) -> Option<String> {
    match v {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

impl Importer for ResultsJsonImporter {
    fn kind(&self) -> ImporterKind {
        ImporterKind::Evidence(self.format)
    }
    fn version(&self) -> u32 {
        1
    }
    fn import(&self, ctx: &ImportContext, input: &ImportInput) -> ImportOutput {
        let mut out = ImportOutput::default();
        let name = self.format.as_str();
        let doc: Doc = match serde_json::from_slice(&input.bytes) {
            Ok(d) => d,
            Err(e) => {
                out.fatal(
                    &input.rel,
                    name,
                    format!("not a results document: {e}"),
                    Some(format!("line {}", e.line())),
                );
                return out;
            }
        };
        if doc.schema.as_deref() != Some("cockpit.results.v1") && doc.contract != Some(1) {
            out.fatal(&input.rel, name, "neither schema cockpit.results.v1 nor contract 1", None);
            return out;
        }
        let prov = doc.provenance.as_ref();
        let decl = Declared {
            commit: prov.and_then(|p| p.commit.clone()).or(doc.commit.clone()),
            branch: prov.and_then(|p| p.branch.clone()).or(doc.branch.clone()),
            timestamp: prov
                .and_then(|p| p.generated_at.as_deref())
                .and_then(parse_timestamp)
                .or_else(|| doc.finished_at.as_deref().and_then(parse_timestamp)),
            tool: prov.and_then(|p| p.tool.as_ref()).or(doc.tool.as_ref()).and_then(|t| {
                t.name.clone().map(|n| ToolInfo { name: n, version: t.version.clone() })
            }),
            run_id: prov
                .and_then(|p| p.run.as_ref())
                .and_then(|r| val_string(&r.id))
                .or_else(|| val_string(&doc.run_id)),
            run_url: prov
                .and_then(|p| p.run.as_ref())
                .and_then(|r| r.url.clone())
                .or(doc.run_url.clone()),
            dirty: prov.and_then(|p| p.dirty),
            repo: None,
        };
        if decl.timestamp.is_none() {
            out.warn(
                &input.rel,
                name,
                "no finished_at / generated_at; using the file's modification time",
                None,
            );
        }
        let base_dir = input.rel.parent();
        let declared_id_early = match (&doc.source, &doc.suite) {
            (Some(s), Some(u)) => Some(format!("{s}/{u}")),
            _ => None,
        };
        let all_sources_early = ctx.located.evidence_sources();
        let matched_early = declared_id_early.as_ref().and_then(|id| {
            all_sources_early.iter().find(|s| s.id.as_deref() == Some(id.as_str())).cloned()
        });
        let effective_src = matched_early.as_ref().or(ctx.source);
        let by_level = effective_src.map(|s| s.component_by_level.clone()).unwrap_or_default();
        let default_kind = doc
            .kind
            .as_deref()
            .map(TestKind::parse)
            .or_else(|| effective_src.map(|s| TestKind::parse(s.default_kind())))
            .unwrap_or(TestKind::Unit);
        let mut results = Vec::new();
        for r in &doc.results {
            let status = match r.status.as_deref() {
                Some("passed") => TestStatus::Passed,
                Some("failed") => TestStatus::Failed,
                Some("skipped") => TestStatus::Skipped,
                Some("flaky") => TestStatus::Flaky,
                other => {
                    out.warn(
                        &input.rel,
                        name,
                        format!(
                            "result {:?} has unknown status {other:?}; treated as skipped",
                            r.test_id
                        ),
                        None,
                    );
                    TestStatus::Skipped
                }
            };
            let component = r
                .component
                .clone()
                .or_else(|| r.level.as_ref().and_then(|l| by_level.get(l).cloned()))
                .or_else(|| doc.component.clone())
                .map(ComponentId::new);
            let mut declared: Vec<String> = r.features.clone();
            for f in &doc.features {
                if !declared.contains(f) {
                    declared.push(f.clone());
                }
            }
            let mut artifacts = Vec::new();
            for a in &r.artifacts {
                let (path, exists) = match (&a.path, &a.url) {
                    (Some(p), _) => {
                        let rel = base_dir.join(p);
                        let exists = rel.to_abs(&ctx.located.root).exists();
                        (ArtifactPath::Project(rel), exists)
                    }
                    (None, Some(u)) => (ArtifactPath::Url(u.clone()), true),
                    _ => continue,
                };
                let key = match &path {
                    ArtifactPath::Project(p) => p.as_str().to_string(),
                    ArtifactPath::Url(u) => u.clone(),
                    _ => unreachable!(),
                };
                let role = match a.kind.as_deref() {
                    Some("expected") => ArtifactRole::Expected,
                    Some("actual") => ArtifactRole::Actual,
                    Some("diff") => ArtifactRole::Diff,
                    _ => ArtifactRole::Plain,
                };
                artifacts.push(Artifact {
                    id: ArtifactId::derive(&key),
                    kind: match a.kind.as_deref() {
                        Some("screenshot") | Some("expected") | Some("actual") | Some("diff") => {
                            ArtifactKind::Screenshot
                        }
                        Some("video") => ArtifactKind::Video,
                        Some("trace") => ArtifactKind::Trace,
                        Some("log") => ArtifactKind::Log,
                        _ => a
                            .mime
                            .as_deref()
                            .map(ArtifactKind::from_content_type)
                            .unwrap_or(ArtifactKind::Other),
                    },
                    role,
                    name: a
                        .label
                        .clone()
                        .unwrap_or_else(|| key.rsplit('/').next().unwrap_or(&key).to_string()),
                    path,
                    content_type: a.mime.clone(),
                    size: None,
                    exists,
                });
            }
            if let Some(u) = &r.trace_url {
                artifacts.push(Artifact {
                    id: ArtifactId::derive(u),
                    kind: ArtifactKind::Report,
                    role: ArtifactRole::Plain,
                    name: "run".into(),
                    path: ArtifactPath::Url(u.clone()),
                    content_type: None,
                    size: None,
                    exists: true,
                });
            }
            let mut message = r.error.as_ref().and_then(|e| e.message.clone());
            if message.is_none() && status != TestStatus::Passed {
                message = r.note.clone();
            }
            let (file, title) = match r.test_id.split_once("::") {
                Some((f, t)) => (Some(f.to_string()), t.to_string()),
                None => (None, r.test_id.clone()),
            };
            let source_path = r.file.clone().or(file.clone()).map(|f| {
                let root_rel = ctx
                    .source
                    .and_then(|s| s.repo.clone())
                    .map(|repo| {
                        ctx.repos
                            .prefix_of(&crate::ids::RepoId::new(repo))
                            .cloned()
                            .unwrap_or_else(RelPath::root)
                    })
                    .unwrap_or_else(RelPath::root);
                root_rel.join(&f)
            });
            results.push(RawResult {
                key: r.test_id.clone(),
                name: r.title.clone().unwrap_or(title),
                classname: file,
                status: Some(status),
                duration_ms: r.duration_ms.map(|d| d.round() as u64),
                retries: r.retries.unwrap_or(0),
                timestamp: None,
                message,
                details: r
                    .error
                    .as_ref()
                    .and_then(|e| e.stack.clone())
                    .or_else(|| r.document.as_ref().map(|d| format!("document: {d}"))),
                tags: r
                    .level
                    .iter()
                    .map(|l| format!("level:{l}"))
                    .chain(r.document.iter().map(|d| format!("document:{d}")))
                    .collect(),
                declared_feature_ids: declared,
                source_path,
                source_path_origin: Some(crate::time::Origin::Declared),
                kind: None,
                component,
                level: r.level.clone(),
                artifacts,
                ..Default::default()
            });
        }
        // A payload that names its own (source, suite) belongs to the configured evidence source
        // of that name when one exists — e.g. a plugin-doc/nextest payload cached under state/.
        let declared_id = match (&doc.source, &doc.suite) {
            (Some(s), Some(u)) => Some(format!("{s}/{u}")),
            _ => None,
        };
        let all_sources = ctx.located.evidence_sources();
        let matched_cfg = declared_id.as_ref().and_then(|id| {
            all_sources.iter().find(|s| s.id.as_deref() == Some(id.as_str())).cloned()
        });
        let source_override: Option<crate::ids::SourceId> = match (&declared_id, &matched_cfg) {
            (Some(id), Some(_))
                if ctx.source_id.as_ref().map(|s| s.as_str() != id).unwrap_or(true) =>
            {
                Some(crate::ids::SourceId::new(id.clone()))
            }
            _ => None,
        };
        let cfg_for_repo = matched_cfg.as_ref().or(ctx.source);
        let mut decl = decl;
        if decl.repo.is_none() {
            decl.repo = cfg_for_repo
                .and_then(|s| s.repo.clone())
                .or_else(|| {
                    doc.source
                        .as_ref()
                        .filter(|s| ctx.project.repos.iter().any(|r| r.id.as_str() == s.as_str()))
                        .cloned()
                })
                .map(crate::ids::RepoId::new);
        }
        let suite_name = matched_cfg
            .as_ref()
            .and_then(|s| s.suite.clone())
            .or_else(|| ctx.source.and_then(|s| s.suite.clone()))
            .or(doc.suite.clone())
            .unwrap_or_else(|| "results".into());
        let run_identity = decl
            .run_id
            .clone()
            .map(|r| format!("{}:{}:{r}", doc.source.clone().unwrap_or_default(), suite_name));
        let matched_ref = matched_cfg.as_ref();
        let (batch, notes) = assemble(
            ctx,
            BatchSpec {
                format: self.format,
                version: self.version(),
                suite_name,
                default_kind,
                declared: decl,
                run_identity,
                results,
                input,
                source_override: source_override.map(|id| (id, matched_ref)),
            },
        );
        for n in notes {
            out.warn(&input.rel, name, n, None);
        }
        out.entities.push(ImportedEntity::TestBatch(batch));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::feature_md::tests::{input, located, project};
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn paged_results_v1_and_cockpit_results() {
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
        let paged = r#"{"contract":1,"source":"editor","suite":"playwright-e2e","commit":"d4e5f6a","branch":"main","run_id":"9182736455","finished_at":"2026-06-06T14:32:11Z",
          "results":[{"test_id":"e2e/gestures.spec.ts::shift resize keeps aspect ratio","features":["gesture.resize.shift-constrain"],"status":"passed","duration_ms":812,"retries":0,"trace_url":"https://x/runs/1"},
                     {"test_id":"conf::doc1","features":["a.b"],"status":"failed","level":"rendered","document":"fashion.idml","note":"why"}]}"#;
        let out = ResultsJsonImporter { format: EvidenceFormat::PagedResults }
            .import(&ctx, &input("state/conformance/out/paged-results-editor.json", paged));
        let ImportedEntity::TestBatch(b) = &out.entities[0] else { panic!() };
        assert_eq!(b.tests.len(), 2);
        assert_eq!(b.tests[0].declared_feature_ids, vec!["gesture.resize.shift-constrain"]);
        assert_eq!(b.tests[0].name, "shift resize keeps aspect ratio");
        assert_eq!(b.run.provenance.run_id.as_deref(), Some("9182736455"));
        assert_eq!(b.run.finished_at.value.to_string(), "2026-06-06T14:32:11Z");
        assert_eq!(b.run.counts.failed, 1);
        assert_eq!(b.test_runs[1].message.as_deref(), Some("why"));
        assert_eq!(b.artifacts.len(), 1);

        let cockpit = r#"{"schema":"cockpit.results.v1","source":"core","suite":"nextest","kind":"unit","provenance":{"commit":"abc","generated_at":"2026-09-01T10:00:00Z","tool":{"name":"cargo-nextest","version":"0.9"}},"results":[{"test_id":"x::y","status":"passed","artifacts":[{"type":"screenshot","path":"shots/a.png"}]}]}"#;
        let out = ResultsJsonImporter { format: EvidenceFormat::CockpitResults }
            .import(&ctx, &input(".cockpit/evidence/core/r.json", cockpit));
        let ImportedEntity::TestBatch(b) = &out.entities[0] else { panic!() };
        assert_eq!(b.tests[0].kind, TestKind::Unit);
        assert_eq!(b.run.provenance.tool.as_ref().unwrap().version.as_deref(), Some("0.9"));
        assert!(matches!(b.artifacts[0].path, ArtifactPath::Project(_)));
        assert!(!b.artifacts[0].exists);

        let out = ResultsJsonImporter { format: EvidenceFormat::PagedResults }
            .import(&ctx, &input("x.json", "{\"nope\":1}"));
        assert!(out.warnings[0].fatal);
    }
}
