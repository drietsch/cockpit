//! Criterion.rs output: `target/criterion/<group>/<function>[/<value>]/new/{benchmark,estimates,sample}.json`
//! plus `change/estimates.json` when a previous run existed. One document per `new/` directory;
//! the commit is inferred from the file's mtime (Low confidence) because Criterion records none.

use super::benchmark_json::{Doc, MetricDoc, StatsDoc, ToolDoc, import_doc};
use super::{ImportContext, ImportInput, ImportOutput, Importer, ImporterKind};
use crate::config::EvidenceFormat;
use serde::Deserialize;

pub struct CriterionImporter;

#[derive(Deserialize, Default)]
#[serde(default)]
struct BenchMeta {
    group_id: Option<String>,
    function_id: Option<String>,
    value_str: Option<String>,
    throughput: Option<serde_json::Value>,
    full_id: Option<String>,
    title: Option<String>,
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
struct Estimate {
    point_estimate: Option<f64>,
    standard_error: Option<f64>,
    confidence_interval: Option<Ci>,
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
struct Ci {
    confidence_level: Option<f64>,
    lower_bound: Option<f64>,
    upper_bound: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Estimates {
    mean: Option<Estimate>,
    median: Option<Estimate>,
    std_dev: Option<Estimate>,
    slope: Option<Estimate>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Sample {
    iters: Vec<f64>,
    times: Vec<f64>,
}

impl Importer for CriterionImporter {
    fn kind(&self) -> ImporterKind {
        ImporterKind::Evidence(EvidenceFormat::Criterion)
    }
    fn version(&self) -> u32 {
        1
    }
    fn import(&self, ctx: &ImportContext, input: &ImportInput) -> ImportOutput {
        let mut out = ImportOutput::default();
        let est: Estimates = match serde_json::from_slice(&input.bytes) {
            Ok(e) => e,
            Err(e) => {
                out.fatal(
                    &input.rel,
                    "criterion",
                    format!("not a Criterion estimates file: {e}"),
                    Some(format!("line {}", e.line())),
                );
                return out;
            }
        };
        let dir = input.abs.parent().unwrap_or(&input.abs);
        let meta: BenchMeta = std::fs::read(dir.join("benchmark.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let sample: Option<Sample> = std::fs::read(dir.join("sample.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        let change: Option<Estimates> = dir
            .parent()
            .map(|p| p.join("change").join("estimates.json"))
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok());
        // Name from metadata, else from the directory layout (`<group>/<function>/<value>/new`).
        let rel_parts: Vec<&str> = input.rel.as_str().split('/').collect();
        let crit_idx = rel_parts.iter().rposition(|p| *p == "criterion");
        let path_parts: Vec<&str> = match crit_idx {
            Some(i) => rel_parts[i + 1..rel_parts.len().saturating_sub(2)].to_vec(),
            None => vec![],
        };
        let group = meta
            .group_id
            .clone()
            .or_else(|| path_parts.first().map(|s| s.to_string()))
            .unwrap_or_else(|| "criterion".into());
        let mut subject_parts: Vec<String> = Vec::new();
        if let Some(f) =
            meta.function_id.clone().or_else(|| path_parts.get(1).map(|s| s.to_string()))
        {
            subject_parts.push(f);
        }
        if let Some(v) = meta.value_str.clone().or_else(|| path_parts.get(2).map(|s| s.to_string()))
        {
            subject_parts.push(v);
        }
        let subject = if subject_parts.is_empty() { None } else { Some(subject_parts.join("/")) };
        let _ = (&meta.full_id, &meta.title);
        let median = est.median.as_ref().or(est.mean.as_ref()).and_then(|e| e.point_estimate);
        let Some(value) = median else {
            out.warn(&input.rel, "criterion", "no median or mean point estimate".to_string(), None);
            return out;
        };
        let samples: Vec<f64> = sample
            .as_ref()
            .map(|s| {
                s.iters
                    .iter()
                    .zip(&s.times)
                    .filter(|(i, _)| **i > 0.0)
                    .map(|(i, t)| t / i)
                    .collect()
            })
            .unwrap_or_default();
        let ci =
            est.median.as_ref().or(est.mean.as_ref()).and_then(|e| e.confidence_interval.clone());
        let stats = StatsDoc {
            n: if samples.is_empty() { None } else { Some(samples.len() as u32) },
            mean: est.mean.as_ref().and_then(|e| e.point_estimate),
            median: est.median.as_ref().and_then(|e| e.point_estimate),
            std_dev: est.std_dev.as_ref().and_then(|e| e.point_estimate),
            min: samples.iter().cloned().reduce(f64::min),
            max: samples.iter().cloned().reduce(f64::max),
            ci: ci.map(|c| super::benchmark_json::CiDoc {
                level: c.confidence_level,
                lower: c.lower_bound,
                upper: c.upper_bound,
            }),
            ..Default::default()
        };
        let _ = est.slope;
        let mut metrics = vec![MetricDoc {
            name: "median".into(),
            unit: Some("ns".into()),
            direction: Some("lower_is_better".into()),
            value: Some(value),
            samples: samples.iter().copied().take(10_000).collect(),
            statistics: Some(stats),
            ..Default::default()
        }];
        if let Some(t) = &meta.throughput {
            let (n, unit) = match t {
                serde_json::Value::Object(m) => {
                    if let Some(b) = m.get("Bytes").and_then(|v| v.as_f64()) {
                        (Some(b), "bytes/s")
                    } else if let Some(e) = m.get("Elements").and_then(|v| v.as_f64()) {
                        (Some(e), "elements/s")
                    } else {
                        (None, "")
                    }
                }
                _ => (None, ""),
            };
            if let Some(n) = n {
                if value > 0.0 {
                    metrics.push(MetricDoc {
                        name: "throughput".into(),
                        unit: Some(unit.into()),
                        direction: Some("higher_is_better".into()),
                        value: Some(n / (value / 1e9)),
                        ..Default::default()
                    });
                }
            }
        }
        let reported_change =
            change.as_ref().and_then(|c| c.median.as_ref().or(c.mean.as_ref())).and_then(|e| {
                let rel = e.point_estimate?;
                let significant = e.confidence_interval.as_ref().and_then(|ci| {
                    let lo = ci.lower_bound?;
                    let hi = ci.upper_bound?;
                    Some(lo > 0.0 || hi < 0.0)
                });
                Some((rel, significant))
            });
        let doc = Doc {
            schema: Some("criterion".into()),
            name: Some(group),
            tool: Some(ToolDoc { name: Some("criterion".into()), version: None }),
            comparability: vec!["machine.os".into(), "machine.arch".into(), "tool".into()],
            metrics: metrics
                .into_iter()
                .map(|mut m| {
                    m.subject = subject.clone();
                    m
                })
                .collect(),
            ..Default::default()
        };
        import_doc(
            ctx,
            input,
            doc,
            EvidenceFormat::Criterion,
            self.version(),
            "criterion",
            &mut out,
        );
        if let Some((rel, significant)) = reported_change {
            for e in out.entities.iter_mut() {
                if let super::ImportedEntity::BenchmarkBatch(b) = e {
                    for r in b.benchmark_runs.iter_mut() {
                        r.measurement.reported_change = Some(crate::model::ReportedChange {
                            relative: rel,
                            significant,
                            note: Some("criterion change/estimates.json".into()),
                        });
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::feature_md::tests::{located, project};
    use super::super::{ImportContext, ImportInput, ImportedEntity, Importer};
    use super::*;
    use crate::hash::ContentHash;
    use crate::paths::RelPath;
    use std::collections::HashSet;

    #[test]
    fn reads_a_criterion_directory_with_change_estimates() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let bench = root.join("target/criterion/parse/small/new");
        std::fs::create_dir_all(&bench).unwrap();
        std::fs::create_dir_all(root.join("target/criterion/parse/small/change")).unwrap();
        std::fs::write(bench.join("benchmark.json"), r#"{"group_id":"parse","function_id":"small","value_str":null,"throughput":{"Bytes":4096},"full_id":"parse/small","directory_name":"parse/small","title":"parse/small"}"#).unwrap();
        std::fs::write(bench.join("estimates.json"), r#"{"mean":{"confidence_interval":{"confidence_level":0.95,"lower_bound":1010.0,"upper_bound":1030.0},"point_estimate":1020.0,"standard_error":5.0},"median":{"confidence_interval":{"confidence_level":0.95,"lower_bound":990.0,"upper_bound":1010.0},"point_estimate":1000.0,"standard_error":4.0},"std_dev":{"confidence_interval":{"confidence_level":0.95,"lower_bound":1.0,"upper_bound":3.0},"point_estimate":2.0,"standard_error":0.1}}"#).unwrap();
        std::fs::write(bench.join("sample.json"), r#"{"sampling_mode":"Linear","iters":[10.0,20.0,30.0],"times":[10000.0,20200.0,29700.0]}"#).unwrap();
        std::fs::write(root.join("target/criterion/parse/small/change/estimates.json"), r#"{"mean":{"confidence_interval":{"confidence_level":0.95,"lower_bound":0.05,"upper_bound":0.09},"point_estimate":0.07,"standard_error":0.01},"median":{"confidence_interval":{"confidence_level":0.95,"lower_bound":0.04,"upper_bound":0.08},"point_estimate":0.06,"standard_error":0.01}}"#).unwrap();
        let mut loc = located();
        loc.root = root.to_path_buf();
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
        let rel = "target/criterion/parse/small/new/estimates.json";
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let input = ImportInput {
            abs: root.join(rel),
            rel: RelPath::new(rel),
            content_hash: ContentHash::of(&bytes),
            size: bytes.len() as u64,
            bytes,
            mtime: crate::time::parse_timestamp("2026-09-11T10:00:00Z").unwrap(),
        };
        let out = CriterionImporter.import(&ctx, &input);
        let ImportedEntity::BenchmarkBatch(b) = &out.entities[0] else {
            panic!("{:?}", out.warnings)
        };
        assert_eq!(b.benchmarks.len(), 2, "median + throughput");
        let median = &b.benchmarks[0];
        assert_eq!(median.name, "parse");
        assert_eq!(median.subject.as_deref(), Some("small"));
        assert_eq!(median.metric.name, "median");
        assert_eq!(median.metric.unit, "ns");
        assert_eq!(median.metric.determinism.as_str(), "environment-sensitive");
        let run = &b.benchmark_runs[0];
        assert_eq!(run.measurement.value, 1000.0);
        let st = run.measurement.statistics.as_ref().unwrap();
        assert_eq!(st.n, Some(3));
        assert_eq!(st.mean, Some(1020.0));
        assert_eq!(st.ci.as_ref().unwrap().upper, 1010.0);
        assert_eq!(run.measurement.samples.as_ref().unwrap()[1], 1010.0);
        let ch = run.measurement.reported_change.as_ref().unwrap();
        assert_eq!(ch.relative, 0.06);
        assert_eq!(ch.significant, Some(true));
        assert_eq!(b.benchmarks[1].metric.name, "throughput");
        assert_eq!(b.benchmarks[1].metric.direction.as_str(), "higher_is_better");
        assert!((b.benchmark_runs[1].measurement.value - 4096.0 / 1e-6).abs() < 1.0);
        assert_eq!(run.timestamp.origin, crate::time::Origin::FileMtime);
    }
}
