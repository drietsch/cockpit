//! Visual evidence importers: paged's fidelity-diff directories (`gate.json` + `report.json` +
//! `cand/ref/heat-NNN.png`) and the normative `cockpit.visual.v1` document. Both funnel
//! through `batch::assemble`: one test per fixture/comparison, one visual per page.

use super::batch::{BatchSpec, RawResult, RawVisual, assemble};
use super::provenance::Declared;
use super::{ImportContext, ImportInput, ImportOutput, ImportedEntity, Importer, ImporterKind};
use crate::config::EvidenceFormat;
use crate::ids::ArtifactId;
use crate::model::*;
use crate::paths::RelPath;
use crate::time::parse_timestamp;
use serde::Deserialize;

fn image(ctx: &ImportContext, rel: RelPath, role: ArtifactRole) -> Option<Artifact> {
    let abs = rel.to_abs(&ctx.located.root);
    let meta = std::fs::metadata(&abs).ok()?;
    Some(Artifact {
        id: ArtifactId::derive(rel.as_str()),
        kind: ArtifactKind::Image,
        role,
        name: rel.file_name().to_string(),
        path: ArtifactPath::Project(rel),
        content_type: Some(mime_guess::from_path(&abs).first_or_octet_stream().to_string()),
        size: Some(meta.len()),
        exists: true,
    })
}

// ── fidelity-diff ────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(default)]
struct Gate {
    name: Option<String>,
    fixture: Option<String>,
    pages_checked: Option<u32>,
    pages_total: Option<u32>,
    passed: Option<bool>,
    skipped: Option<bool>,
    reason: Option<String>,
    thresholds: Option<Thresholds>,
    failures: Vec<serde_json::Value>,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(default)]
struct Thresholds {
    max_mean_de: Option<f64>,
    max_p99_de: Option<f64>,
    min_ssim: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PageReport {
    page: u32,
    mean_de: Option<f64>,
    p99_de: Option<f64>,
    max_de: Option<f64>,
    ssim: Option<f64>,
    passes: Option<bool>,
}

pub struct FidelityImporter;

impl Importer for FidelityImporter {
    fn kind(&self) -> ImporterKind {
        ImporterKind::Evidence(EvidenceFormat::Fidelity)
    }
    fn version(&self) -> u32 {
        1
    }
    fn import(&self, ctx: &ImportContext, input: &ImportInput) -> ImportOutput {
        let mut out = ImportOutput::default();
        let gate: Gate = match serde_json::from_slice(&input.bytes) {
            Ok(g) => g,
            Err(e) => {
                out.fatal(
                    &input.rel,
                    "fidelity",
                    format!("not a fidelity gate: {e}"),
                    Some(format!("line {}", e.line())),
                );
                return out;
            }
        };
        let dir = input.rel.parent();
        let fixture = gate
            .name
            .clone()
            .or_else(|| gate.fixture.clone())
            .unwrap_or_else(|| dir.file_name().to_string());
        let report: Vec<PageReport> =
            std::fs::read(dir.join("report.json").to_abs(&ctx.located.root))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
        let skipped = gate.skipped == Some(true);
        let pages_checked = gate.pages_checked.unwrap_or(report.len() as u32);
        let failing_pages: Vec<u32> = gate
            .failures
            .iter()
            .filter_map(|f| f.get("page").and_then(|p| p.as_u64()).map(|p| p as u32))
            .collect();
        let failure_reasons: Vec<String> = gate
            .failures
            .iter()
            .map(|f| {
                let page = f
                    .get("page")
                    .and_then(|p| p.as_u64())
                    .map(|p| format!("page {p}: "))
                    .unwrap_or_default();
                let v = f
                    .get("violations")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))
                    .or_else(|| f.get("reason").and_then(|r| r.as_str()).map(|s| s.to_string()))
                    .unwrap_or_default();
                format!("{page}{v}")
            })
            .collect();
        let (status, message) = if skipped {
            (TestStatus::Skipped, gate.reason.clone().or_else(|| Some("skipped".into())))
        } else if pages_checked == 0 {
            (
                TestStatus::Skipped,
                Some(
                    failure_reasons
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "no pages compared — nothing was measured".into()),
                ),
            )
        } else if gate.passed == Some(false) || !failing_pages.is_empty() {
            (TestStatus::Failed, Some(failure_reasons.join("; ")))
        } else {
            (TestStatus::Passed, None)
        };
        let thresholds_json = gate.thresholds.map(|t| serde_json::json!({ "max_mean_de": t.max_mean_de, "max_p99_de": t.max_p99_de, "min_ssim": t.min_ssim }));
        let mut visuals = Vec::new();
        if !skipped {
            let mut pages: Vec<&PageReport> = report.iter().collect();
            pages.sort_by_key(|p| p.page);
            for p in pages {
                let n = format!("{:03}", p.page);
                let cand = image(ctx, dir.join(&format!("cand-{n}.png")), ArtifactRole::Actual);
                let reference =
                    image(ctx, dir.join(&format!("ref-{n}.png")), ArtifactRole::Expected);
                let heat = image(ctx, dir.join(&format!("heat-{n}.png")), ArtifactRole::Heat);
                let within = |t: Thresholds| -> Option<bool> {
                    let mut ok = true;
                    let mut any = false;
                    if let (Some(v), Some(m)) = (p.mean_de, t.max_mean_de) {
                        any = true;
                        ok &= v <= m;
                    }
                    if let (Some(v), Some(m)) = (p.p99_de, t.max_p99_de) {
                        any = true;
                        ok &= v <= m;
                    }
                    if let (Some(v), Some(m)) = (p.ssim, t.min_ssim) {
                        any = true;
                        ok &= v >= m;
                    }
                    any.then_some(ok)
                };
                let vstatus = if failing_pages.contains(&p.page) {
                    VisualStatus::Regression
                } else if gate.thresholds.is_some() && pages_checked >= p.page {
                    match gate.thresholds.and_then(within) {
                        Some(true) => VisualStatus::Current,
                        Some(false) => VisualStatus::Regression,
                        None => VisualStatus::Inconclusive,
                    }
                } else if pages_checked < p.page {
                    VisualStatus::Skipped
                } else if p.passes == Some(true) {
                    VisualStatus::Current
                } else if reference.is_none() {
                    VisualStatus::Missing
                } else {
                    VisualStatus::Inconclusive
                };
                visuals.push(RawVisual {
                    name: format!("page {}", p.page),
                    page: Some(p.page),
                    baseline: reference,
                    actual: cand,
                    diff: heat,
                    metrics: DiffMetrics {
                        changed_pixels: None,
                        difference_ratio: None,
                        mean_de: p.mean_de,
                        p99_de: p.p99_de,
                        max_de: p.max_de,
                        ssim: p.ssim,
                        thresholds: thresholds_json.clone(),
                    },
                    status: vstatus,
                });
            }
        }
        let _ = gate.pages_total;
        let results = vec![RawResult {
            key: format!("fidelity/{fixture}"),
            name: fixture.clone(),
            classname: Some("fidelity".into()),
            status: Some(status),
            duration_ms: None,
            retries: 0,
            timestamp: None,
            message,
            details: None,
            tags: vec![],
            declared_feature_ids: ctx.source.map(|s| s.features.clone()).unwrap_or_default(),
            source_path: None,
            source_path_origin: None,
            kind: Some(TestKind::Visual),
            component: None,
            level: None,
            browser: None,
            viewport: None,
            artifacts: vec![],
            visuals,
        }];
        let (batch, notes) = assemble(
            ctx,
            BatchSpec {
                format: EvidenceFormat::Fidelity,
                version: self.version(),
                suite_name: ctx
                    .source
                    .and_then(|s| s.suite.clone())
                    .unwrap_or_else(|| "fidelity".into()),
                default_kind: TestKind::Visual,
                declared: Declared::default(),
                run_identity: Some(format!("{}:{}", fixture, input.content_hash.hex())),
                results,
                input,
                source_override: None,
            },
        );
        for n in notes {
            out.warn(&input.rel, "fidelity", n, None);
        }
        out.entities.push(ImportedEntity::TestBatch(batch));
        out
    }
}

// ── cockpit.visual.v1 ────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(default)]
struct VisualDoc {
    schema: Option<String>,
    source: Option<String>,
    suite: Option<String>,
    provenance: Option<super::benchmark_json::ProvDoc>,
    features: Vec<String>,
    comparisons: Vec<Comparison>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Comparison {
    id: String,
    title: Option<String>,
    feature_ids: Vec<String>,
    #[serde(alias = "features")]
    features_alias: Vec<String>,
    component: Option<String>,
    page: Option<u32>,
    browser: Option<String>,
    viewport: Option<Viewport>,
    baseline: Option<ImageRef>,
    actual: Option<ImageRef>,
    diff: Option<ImageRef>,
    metrics: Option<DiffMetrics>,
    threshold: Option<serde_json::Value>,
    status: Option<String>,
    reason: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ImageRef {
    path: Option<String>,
    url: Option<String>,
}

pub struct CockpitVisualImporter;

impl Importer for CockpitVisualImporter {
    fn kind(&self) -> ImporterKind {
        ImporterKind::Evidence(EvidenceFormat::CockpitVisual)
    }
    fn version(&self) -> u32 {
        1
    }
    fn import(&self, ctx: &ImportContext, input: &ImportInput) -> ImportOutput {
        let mut out = ImportOutput::default();
        let doc: VisualDoc = match serde_json::from_slice(&input.bytes) {
            Ok(d) => d,
            Err(e) => {
                out.fatal(
                    &input.rel,
                    "cockpit-visual",
                    format!("not a cockpit.visual document: {e}"),
                    Some(format!("line {}", e.line())),
                );
                return out;
            }
        };
        if doc.schema.as_deref().map(|s| !s.starts_with("cockpit.visual")).unwrap_or(false) {
            out.warn(
                &input.rel,
                "cockpit-visual",
                format!("unexpected schema {:?}", doc.schema),
                None,
            );
        }
        let base_dir = input.rel.parent();
        let prov = doc.provenance.as_ref();
        let decl = Declared {
            commit: prov.and_then(|p| p.commit.clone()),
            branch: prov.and_then(|p| p.branch.clone()),
            timestamp: prov.and_then(|p| p.generated_at.as_deref()).and_then(parse_timestamp),
            tool: prov.and_then(|p| p.tool.as_ref()).and_then(|t| {
                t.name.clone().map(|n| ToolInfo { name: n, version: t.version.clone() })
            }),
            run_id: prov.and_then(|p| p.run.as_ref()).and_then(|r| match &r.id {
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                Some(serde_json::Value::Number(n)) => Some(n.to_string()),
                _ => None,
            }),
            run_url: prov.and_then(|p| p.run.as_ref()).and_then(|r| r.url.clone()),
            dirty: prov.and_then(|p| p.dirty),
            repo: None,
        };
        let img = |r: &Option<ImageRef>, role: ArtifactRole| -> Option<Artifact> {
            let r = r.as_ref()?;
            if let Some(p) = &r.path {
                let rel = base_dir.join(p);
                let abs = rel.to_abs(&ctx.located.root);
                let exists = abs.exists();
                return Some(Artifact {
                    id: ArtifactId::derive(rel.as_str()),
                    kind: ArtifactKind::Image,
                    role,
                    name: rel.file_name().to_string(),
                    content_type: Some(
                        mime_guess::from_path(&abs).first_or_octet_stream().to_string(),
                    ),
                    size: std::fs::metadata(&abs).ok().map(|m| m.len()),
                    path: ArtifactPath::Project(rel),
                    exists,
                });
            }
            let u = r.url.clone()?;
            Some(Artifact {
                id: ArtifactId::derive(&u),
                kind: ArtifactKind::Image,
                role,
                name: u.rsplit('/').next().unwrap_or(&u).to_string(),
                path: ArtifactPath::Url(u),
                content_type: None,
                size: None,
                exists: true,
            })
        };
        let mut results = Vec::new();
        for c in &doc.comparisons {
            let baseline = img(&c.baseline, ArtifactRole::Expected);
            let actual = img(&c.actual, ArtifactRole::Actual);
            let diff = img(&c.diff, ArtifactRole::Diff);
            let (vstatus, tstatus) = match c.status.as_deref().unwrap_or("inconclusive") {
                "pass" | "current" => (VisualStatus::Current, TestStatus::Passed),
                "fail" | "regression" => (VisualStatus::Regression, TestStatus::Failed),
                "missing_baseline" | "missing" => (VisualStatus::Missing, TestStatus::Skipped),
                "new_baseline" => (VisualStatus::Current, TestStatus::Passed),
                "skipped" => (VisualStatus::Skipped, TestStatus::Skipped),
                _ => (VisualStatus::Inconclusive, TestStatus::Skipped),
            };
            let mut metrics = c.metrics.clone().unwrap_or_default();
            if metrics.thresholds.is_none() {
                metrics.thresholds = c.threshold.clone();
            }
            let mut feats = c.feature_ids.clone();
            feats.extend(c.features_alias.iter().cloned());
            feats.extend(doc.features.iter().cloned());
            results.push(RawResult {
                key: c.id.clone(),
                name: c.title.clone().unwrap_or_else(|| c.id.clone()),
                classname: None,
                status: Some(tstatus),
                duration_ms: None,
                retries: 0,
                timestamp: None,
                message: c.reason.clone(),
                details: None,
                tags: vec![],
                declared_feature_ids: feats,
                source_path: None,
                source_path_origin: None,
                kind: Some(TestKind::Visual),
                component: c.component.clone().map(crate::ids::ComponentId::new),
                level: None,
                browser: c.browser.clone(),
                viewport: c.viewport,
                artifacts: vec![],
                visuals: vec![RawVisual {
                    name: c.title.clone().unwrap_or_else(|| c.id.clone()),
                    page: c.page,
                    baseline,
                    actual,
                    diff,
                    metrics,
                    status: vstatus,
                }],
            });
        }
        let all_sources = ctx.located.evidence_sources();
        let declared_id = match (&doc.source, &doc.suite) {
            (Some(s), Some(u)) => Some(format!("{s}/{u}")),
            _ => None,
        };
        let source_override: Option<(
            crate::ids::SourceId,
            Option<&crate::config::EvidenceSource>,
        )> = declared_id.and_then(|id| {
            all_sources
                .iter()
                .find(|s| s.id.as_deref() == Some(id.as_str()))
                .map(|s| (crate::ids::SourceId::new(id), Some(s)))
        });
        let (batch, notes) = assemble(
            ctx,
            BatchSpec {
                format: EvidenceFormat::CockpitVisual,
                version: self.version(),
                suite_name: doc
                    .suite
                    .clone()
                    .or_else(|| ctx.source.and_then(|s| s.suite.clone()))
                    .unwrap_or_default(),
                default_kind: TestKind::Visual,
                declared: decl,
                run_identity: None,
                results,
                input,
                source_override,
            },
        );
        for n in notes {
            out.warn(&input.rel, "cockpit-visual", n, None);
        }
        out.entities.push(ImportedEntity::TestBatch(batch));
        out
    }
}
