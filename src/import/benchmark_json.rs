//! `cockpit.benchmark.v1` — the normative benchmark document — plus the mappers that turn
//! Guardian's `BenchResult` and paged's measurements contract into the same in-memory shape.

use super::provenance::Declared;
use super::{
    BenchmarkBatch, ImportContext, ImportInput, ImportOutput, ImportedEntity, Importer,
    ImporterKind,
};
use crate::config::EvidenceFormat;
use crate::ids::{BenchmarkId, BenchmarkRunId, ComponentId, RunId, SourceId};
use crate::model::*;
use crate::time::{Origin, Stamped, parse_timestamp};
use serde::Deserialize;

/// The document as parsed (all formats normalise into this).
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct Doc {
    pub schema: Option<String>,
    pub name: Option<String>,
    pub feature_ids: Vec<String>,
    #[serde(alias = "features")]
    pub features_alias: Vec<String>,
    pub component: Option<String>,
    pub provenance: ProvDoc,
    pub generated_at: Option<String>,
    pub build: BuildDoc,
    pub machine: MachineDoc,
    pub environment: EnvDoc,
    pub workload: serde_json::Value,
    pub comparability: Vec<String>,
    pub generator_version: Option<u32>,
    pub shape_version: Option<u32>,
    pub checks: Option<u64>,
    pub tool: Option<ToolDoc>,
    #[serde(alias = "measurements")]
    pub metrics: Vec<MetricDoc>,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct ProvDoc {
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub dirty: Option<bool>,
    pub generated_at: Option<String>,
    pub tool: Option<ToolDoc>,
    pub run: Option<RunDoc>,
    pub environment: Option<EnvDoc>,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct RunDoc {
    pub id: Option<serde_json::Value>,
    pub url: Option<String>,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct ToolDoc {
    pub name: Option<String>,
    pub version: Option<String>,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct BuildDoc {
    pub commit: Option<String>,
    pub dirty: Option<bool>,
    pub rustc: Option<String>,
    pub lock_hash: Option<String>,
    pub release: Option<bool>,
    pub profile: Option<String>,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct MachineDoc {
    pub os: Option<String>,
    pub arch: Option<String>,
    pub cpus: Option<u32>,
    #[serde(alias = "model")]
    pub cpu_model: Option<String>,
    pub runner: Option<String>,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct EnvDoc {
    pub id: Option<String>,
    pub class: Option<String>,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub cpus: Option<u32>,
    pub runner: Option<String>,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct MetricDoc {
    #[serde(alias = "metric")]
    pub name: String,
    pub subject: Option<String>,
    pub dimensions: serde_json::Value,
    pub unit: Option<String>,
    pub direction: Option<String>,
    pub determinism: Option<String>,
    pub value: Option<f64>,
    pub samples: Vec<f64>,
    pub statistics: Option<StatsDoc>,
    pub budget: Option<serde_json::Value>,
    pub status: Option<String>,
    pub reason: Option<String>,
    #[serde(alias = "classification")]
    pub accepted: Option<String>,
    pub features: Vec<String>,
    pub component: Option<String>,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct StatsDoc {
    pub n: Option<u32>,
    pub mean: Option<f64>,
    pub median: Option<f64>,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
    #[serde(alias = "stddev")]
    pub std_dev: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub ci95: Option<[f64; 2]>,
    pub ci: Option<CiDoc>,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct CiDoc {
    pub level: Option<f64>,
    pub lower: Option<f64>,
    pub upper: Option<f64>,
}

pub struct BenchmarkJsonImporter;

impl Importer for BenchmarkJsonImporter {
    fn kind(&self) -> ImporterKind {
        ImporterKind::Evidence(EvidenceFormat::CockpitBenchmark)
    }
    fn version(&self) -> u32 {
        1
    }
    fn import(&self, ctx: &ImportContext, input: &ImportInput) -> ImportOutput {
        let mut out = ImportOutput::default();
        let doc: Doc = match serde_json::from_slice(&input.bytes) {
            Ok(d) => d,
            Err(e) => {
                out.fatal(
                    &input.rel,
                    "cockpit-benchmark",
                    format!("not a benchmark document: {e}"),
                    Some(format!("line {}", e.line())),
                );
                return out;
            }
        };
        import_doc(
            ctx,
            input,
            doc,
            EvidenceFormat::CockpitBenchmark,
            self.version(),
            "cockpit-benchmark",
            &mut out,
        );
        out
    }
}

/// Parse a budget: `150`, `{"max": 150}`, `{"min": 0.99}`, `{"max_relative_pct": 10}`.
pub fn parse_budget(v: &serde_json::Value, direction: Direction) -> Option<Budget> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().map(|x| {
            if direction == Direction::HigherIsBetter { Budget::Min(x) } else { Budget::Max(x) }
        }),
        serde_json::Value::Object(m) => {
            if let Some(x) = m.get("max").and_then(|x| x.as_f64()) {
                Some(Budget::Max(x))
            } else if let Some(x) = m.get("min").and_then(|x| x.as_f64()) {
                Some(Budget::Min(x))
            } else {
                m.get("max_relative_pct")
                    .or(m.get("relative_pct"))
                    .and_then(|x| x.as_f64())
                    .map(Budget::MaxRelativePct)
            }
        }
        _ => None,
    }
}

/// Turn a parsed document into a `BenchmarkBatch` (shared by all benchmark formats).
pub fn import_doc(
    ctx: &ImportContext,
    input: &ImportInput,
    doc: Doc,
    format: EvidenceFormat,
    version: u32,
    importer_name: &str,
    out: &mut ImportOutput,
) {
    let name = doc.name.clone().unwrap_or_else(|| input.rel.file_stem().to_string());
    let prov = &doc.provenance;
    let commit = prov.commit.clone().or_else(|| doc.build.commit.clone());
    let timestamp =
        prov.generated_at.as_deref().or(doc.generated_at.as_deref()).and_then(parse_timestamp);
    let decl =
        Declared {
            commit,
            branch: prov.branch.clone(),
            timestamp,
            tool: prov.tool.as_ref().or(doc.tool.as_ref()).and_then(|t| {
                t.name.clone().map(|n| ToolInfo { name: n, version: t.version.clone() })
            }),
            run_id: prov.run.as_ref().and_then(|r| match &r.id {
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                Some(serde_json::Value::Number(n)) => Some(n.to_string()),
                _ => None,
            }),
            run_url: prov.run.as_ref().and_then(|r| r.url.clone()),
            dirty: prov.dirty.or(doc.build.dirty),
            repo: None,
        };
    let mut built = super::provenance::build(
        ctx,
        format,
        version,
        &input.rel,
        &input.abs,
        input.content_hash,
        input.mtime,
        decl,
    );
    // A checked-in result without a timestamp is dated by the commit that introduced the file,
    // not by a clone's mtime (Guardian's perf/results pattern).
    if built.timestamp.origin == Origin::FileMtime {
        if let Some(c) = built
            .provenance
            .commit
            .as_ref()
            .filter(|c| c.resolved && c.origin == CommitOrigin::Declared)
        {
            if let Some(at) = ctx.commits.commit_time(&c.key.repo, &c.key.sha) {
                built.timestamp = Stamped::new(at, Origin::CommitDate);
                built.provenance.timestamp = Some(built.timestamp);
            }
        }
    }
    for n in &built.notes {
        out.warn(&input.rel, importer_name, n.clone(), None);
    }
    let source_id =
        ctx.source_id.clone().unwrap_or_else(|| SourceId::new(format!("auto:{}", format.as_str())));
    let identity = built.provenance.run_id.clone().unwrap_or_else(|| input.content_hash.hex());
    let run_id = RunId::derive(&source_id, &identity);

    // Environment fingerprint.
    let env = prov.environment.as_ref().unwrap_or(&doc.environment);
    let os = doc.machine.os.clone().or_else(|| env.os.clone());
    let arch = doc.machine.arch.clone().or_else(|| env.arch.clone());
    let cpus = doc.machine.cpus.or(env.cpus);
    let profile = doc.build.profile.clone().or_else(|| {
        doc.build.release.map(|r| if r { "release".to_string() } else { "debug".to_string() })
    });
    let mut extra = serde_json::Map::new();
    if let Some(v) = doc.generator_version {
        extra.insert("generator_version".into(), v.into());
    }
    if let Some(v) = doc.shape_version {
        extra.insert("shape_version".into(), v.into());
    }
    if let Some(h) = &doc.build.lock_hash {
        extra.insert("lock_hash".into(), h.clone().into());
    }
    if let Some(id) = &env.id {
        extra.insert("environment_id".into(), id.clone().into());
    }
    let fingerprint = EnvironmentFingerprint::build(
        os,
        arch,
        cpus,
        doc.machine.cpu_model.clone(),
        doc.machine.runner.clone().or_else(|| env.runner.clone()),
        profile,
        doc.build.rustc.clone(),
        built.provenance.tool.clone(),
        doc.workload.clone(),
        if doc.comparability.is_empty() {
            vec![
                "machine.os".into(),
                "machine.arch".into(),
                "machine.cpus".into(),
                "build.profile".into(),
                "workload".into(),
            ]
        } else {
            doc.comparability.clone()
        },
        serde_json::Value::Object(extra),
    );
    let component = doc
        .component
        .clone()
        .or_else(|| ctx.source.and_then(|s| s.component.clone()))
        .map(ComponentId::new);
    let mut doc_features = doc.feature_ids.clone();
    doc_features.extend(doc.features_alias.iter().cloned());
    if let Some(s) = ctx.source {
        doc_features.extend(s.features.iter().cloned());
    }
    let overrides = &ctx.located.config.performance.metrics;
    let mut benchmarks = Vec::new();
    let mut runs = Vec::new();
    for m in &doc.metrics {
        let Some(value) = m.value else {
            out.warn(&input.rel, importer_name, format!("metric {:?} has no value", m.name), None);
            continue;
        };
        let unit = m.unit.clone().unwrap_or_default();
        let key = format!("{name}/{}/{}", m.subject.clone().unwrap_or_default(), m.name);
        let ov = overrides.iter().find(|o| {
            globset::Glob::new(&o.matcher)
                .ok()
                .map(|g| g.compile_matcher().is_match(&key))
                .unwrap_or(false)
        });
        let (direction, direction_origin) = match m.direction.as_deref().and_then(Direction::parse)
        {
            Some(d) => (d, Origin::Declared),
            None => match ov.and_then(|o| o.direction.as_deref()).and_then(Direction::parse) {
                Some(d) => (d, Origin::Config),
                None => (Direction::infer_from_unit(&unit), Origin::Inferred),
            },
        };
        let (determinism, determinism_origin) =
            match m.determinism.as_deref().and_then(Determinism::parse) {
                Some(d) => (d, Origin::Declared),
                None => {
                    match ov.and_then(|o| o.determinism.as_deref()).and_then(Determinism::parse) {
                        Some(d) => (d, Origin::Config),
                        None => (Determinism::infer_from_unit(&unit), Origin::Inferred),
                    }
                }
            };
        let budget = m.budget.as_ref().and_then(|b| parse_budget(b, direction)).or_else(|| {
            ov.and_then(|o| o.budget.as_ref()).and_then(|b| {
                b.max
                    .map(Budget::Max)
                    .or(b.min.map(Budget::Min))
                    .or(b.max_relative_pct.map(Budget::MaxRelativePct))
            })
        });
        let subject = m.subject.clone().or_else(|| {
            if m.dimensions.is_object() && !m.dimensions.as_object().unwrap().is_empty() {
                Some(crate::model::canonical_json(&m.dimensions))
            } else {
                None
            }
        });
        let bench_id = BenchmarkId::derive(&source_id, &name, subject.as_deref(), &m.name);
        let mut feats = doc_features.clone();
        feats.extend(m.features.iter().cloned());
        benchmarks.push(Benchmark {
            id: bench_id.clone(),
            source: source_id.clone(),
            name: name.clone(),
            subject: subject.clone(),
            metric: MetricDef {
                name: m.name.clone(),
                unit: unit.clone(),
                direction,
                direction_origin,
                determinism,
                determinism_origin,
            },
            budget,
            component: m.component.clone().map(ComponentId::new).or_else(|| component.clone()),
            declared_feature_ids: feats,
            first_seen: built.timestamp.value,
            last_seen: built.timestamp.value,
        });
        let statistics = m.statistics.as_ref().map(|s| Statistics {
            mean: s.mean,
            median: s.median.or(s.p50),
            std_dev: s.std_dev,
            min: s.min,
            max: s.max,
            p50: s.p50.or(s.median),
            p95: s.p95,
            p99: s.p99,
            n: s.n.or(if m.samples.is_empty() { None } else { Some(m.samples.len() as u32) }),
            ci: s
                .ci
                .as_ref()
                .and_then(|c| {
                    Some(ConfidenceInterval {
                        level: c.level.unwrap_or(0.95),
                        lower: c.lower?,
                        upper: c.upper?,
                    })
                })
                .or_else(|| {
                    s.ci95.map(|[lo, hi]| ConfidenceInterval { level: 0.95, lower: lo, upper: hi })
                }),
        });
        runs.push(BenchmarkRun {
            id: BenchmarkRunId::derive(&run_id, &bench_id),
            run: run_id.clone(),
            benchmark: bench_id,
            measurement: Measurement {
                value,
                unit,
                status: m
                    .status
                    .as_deref()
                    .map(MeasurementStatus::parse)
                    .unwrap_or(MeasurementStatus::Ok),
                reason: m.reason.clone(),
                statistics,
                samples: if m.samples.is_empty() {
                    None
                } else {
                    Some(m.samples.iter().copied().take(10_000).collect())
                },
                accepted: m.accepted.clone(),
                reported_change: None,
            },
            commit: built.provenance.commit.clone(),
            branch: built.provenance.branch.clone(),
            dirty: built.provenance.dirty,
            timestamp: built.timestamp,
            environment: fingerprint.id.clone(),
            env_class: fingerprint.class.clone(),
            budget,
        });
    }
    let n = runs.len() as u32;
    let run = Run {
        id: run_id,
        source: source_id,
        suite: None,
        kind: RunKind::Benchmark,
        provenance: {
            let mut p = built.provenance;
            p.environment = Some(fingerprint.id.clone());
            p
        },
        finished_at: built.timestamp,
        started_at: None,
        counts: RunCounts { passed: n, ..Default::default() },
        silent: n == 0,
        silent_reason: if n == 0 { Some("no measurements".into()) } else { None },
    };
    out.entities.push(ImportedEntity::BenchmarkBatch(BenchmarkBatch {
        run,
        environment: fingerprint,
        benchmarks,
        benchmark_runs: runs,
    }));
}

/// Guardian `BenchResult` → `Doc`.
pub struct GuardianBenchImporter;

impl Importer for GuardianBenchImporter {
    fn kind(&self) -> ImporterKind {
        ImporterKind::Evidence(EvidenceFormat::GuardianBench)
    }
    fn version(&self) -> u32 {
        1
    }
    fn import(&self, ctx: &ImportContext, input: &ImportInput) -> ImportOutput {
        let mut out = ImportOutput::default();
        #[derive(Deserialize)]
        struct BenchResult {
            name: String,
            #[serde(default)]
            generator_version: Option<u32>,
            #[serde(default)]
            shape_version: Option<u32>,
            #[serde(default)]
            workload: serde_json::Value,
            #[serde(default)]
            build: BuildDoc,
            #[serde(default)]
            machine: MachineDoc,
            #[serde(default)]
            measurements: Vec<GuardianMeasurement>,
            #[serde(default)]
            checks: Option<u64>,
        }
        #[derive(Deserialize)]
        struct GuardianMeasurement {
            metric: String,
            value: f64,
            unit: String,
            #[serde(default)]
            determinism: Option<String>,
        }
        let r: BenchResult = match serde_json::from_slice(&input.bytes) {
            Ok(r) => r,
            Err(e) => {
                out.fatal(
                    &input.rel,
                    "guardian-bench",
                    format!("not a Guardian BenchResult: {e}"),
                    Some(format!("line {}", e.line())),
                );
                return out;
            }
        };
        let doc = Doc {
            schema: Some("guardian.bench-result".into()),
            name: Some(r.name),
            generator_version: r.generator_version,
            shape_version: r.shape_version,
            workload: r.workload,
            build: r.build,
            machine: r.machine,
            checks: r.checks,
            tool: Some(ToolDoc { name: Some("guardian-testkit".into()), version: None }),
            comparability: vec![
                "machine.os".into(),
                "machine.arch".into(),
                "machine.cpus".into(),
                "build.profile".into(),
                "workload".into(),
            ],
            metrics: r
                .measurements
                .into_iter()
                .map(|m| MetricDoc {
                    name: m.metric,
                    value: Some(m.value),
                    direction: Some(if m.unit.contains("/s") {
                        "higher_is_better".into()
                    } else {
                        "lower_is_better".into()
                    }),
                    determinism: m.determinism,
                    unit: Some(m.unit),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        import_doc(
            ctx,
            input,
            doc,
            EvidenceFormat::GuardianBench,
            self.version(),
            "guardian-bench",
            &mut out,
        );
        out
    }
}

/// paged `paged-measurements-*.json` (contract 1) → `Doc`.
pub struct PagedMeasurementsImporter;

impl Importer for PagedMeasurementsImporter {
    fn kind(&self) -> ImporterKind {
        ImporterKind::Evidence(EvidenceFormat::PagedMeasurements)
    }
    fn version(&self) -> u32 {
        1
    }
    fn import(&self, ctx: &ImportContext, input: &ImportInput) -> ImportOutput {
        let mut out = ImportOutput::default();
        #[derive(Deserialize)]
        struct Payload {
            #[serde(default)]
            source: Option<String>,
            #[serde(default)]
            lane: Option<String>,
            #[serde(default)]
            commit: Option<String>,
            #[serde(default)]
            branch: Option<String>,
            #[serde(default)]
            run_id: Option<serde_json::Value>,
            #[serde(default)]
            finished_at: Option<String>,
            #[serde(default)]
            environment: EnvDoc,
            #[serde(default)]
            measurements: Vec<PagedMeasurement>,
        }
        #[derive(Deserialize)]
        struct PagedMeasurement {
            metric: String,
            #[serde(default)]
            subject: Option<String>,
            value: f64,
            #[serde(default)]
            unit: Option<String>,
            #[serde(default)]
            status: Option<String>,
            #[serde(default)]
            budget: Option<f64>,
            #[serde(default)]
            reason: Option<String>,
        }
        let p: Payload = match serde_json::from_slice(&input.bytes) {
            Ok(p) => p,
            Err(e) => {
                out.fatal(
                    &input.rel,
                    "paged-measurements",
                    format!("not a paged measurements payload: {e}"),
                    Some(format!("line {}", e.line())),
                );
                return out;
            }
        };
        let doc = Doc {
            schema: Some("paged.measurements.v1".into()),
            name: Some(p.lane.clone().unwrap_or_else(|| "measurements".into())),
            provenance: ProvDoc {
                commit: p.commit,
                branch: p.branch,
                generated_at: p.finished_at,
                run: Some(RunDoc { id: p.run_id, url: None }),
                tool: p.source.map(|s| ToolDoc { name: Some(s), version: None }),
                ..Default::default()
            },
            machine: MachineDoc {
                os: p.environment.os.clone(),
                arch: p.environment.arch.clone(),
                cpus: p.environment.cpus,
                ..Default::default()
            },
            metrics: p
                .measurements
                .into_iter()
                .map(|m| MetricDoc {
                    name: m.metric,
                    subject: m.subject,
                    value: Some(m.value),
                    unit: m.unit,
                    status: m.status,
                    reason: m.reason,
                    budget: m.budget.map(|b| serde_json::json!(b)),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        import_doc(
            ctx,
            input,
            doc,
            EvidenceFormat::PagedMeasurements,
            self.version(),
            "paged-measurements",
            &mut out,
        );
        out
    }
}
