mod support;

use cockpit::model::{HealthState, RollupStatus, Severity};
use support::ProjectBuilder;

/// One feature with a `cockpit.benchmark.v1` series checked into `perf/`; every document names
/// the commit it was built from, so freshness and baselines are exact.
const CONFIG: &str = r#"
[[evidence]]
id = "bench"
format = "cockpit-benchmark"
kind = "benchmark"
path = "perf/*.json"

[health]
required_evidence = { critical = ["unit"], high = [], normal = [], low = [] }
"#;

fn doc(commit: &str, at: &str, calls: f64, p50: f64, cpus: u32, budget: Option<f64>) -> String {
    let budget = budget.map(|b| format!(r#", "budget": {{"max": {b}}}"#)).unwrap_or_default();
    format!(
        r#"{{
  "schema": "cockpit.benchmark.v1",
  "name": "store",
  "feature_ids": ["FEAT-001"],
  "provenance": {{ "commit": "{commit}", "branch": "main", "generated_at": "{at}", "dirty": false }},
  "build": {{ "profile": "release", "rustc": "rustc 1.98.0" }},
  "machine": {{ "os": "macos", "arch": "aarch64", "cpus": {cpus} }},
  "workload": {{ "backend": "memory", "products": 2000 }},
  "metrics": [
    {{ "name": "commit.calls", "unit": "calls", "value": {calls}, "determinism": "deterministic"{budget} }},
    {{ "name": "read.p50", "unit": "us", "value": {p50}, "determinism": "environment-sensitive", "samples": [1.0, 2.0, 3.0] }}
  ]
}}"#
    )
}

fn base() -> ProjectBuilder {
    ProjectBuilder::new()
        .config(CONFIG)
        .file("Cargo.toml", "[package]\nname = \"app\"\nversion = \"0.1.0\"\n")
        .file("crates/store/src/lib.rs", "fn a() {}\n")
        .feature(
            "FEAT-001",
            "Storage engine",
            &["crates/store/**"],
            "importance: high\n",
            "Stores.",
        )
        .commit("Add store", "2026-03-01T09:00:00Z")
}

fn perf(c: &cockpit::cockpit::Cockpit) -> cockpit::model::KindRollup {
    let st = c.state.load();
    st.feature("FEAT-001")
        .unwrap()
        .health
        .rollups
        .iter()
        .find(|r| r.category.as_str() == "performance")
        .cloned()
        .unwrap()
}

#[test]
fn first_run_then_regression_then_incomparable_then_breach() {
    let p = base();
    let a = p.head_sha();
    p.put_file("perf/store.json", &doc(&a, "2026-03-01T10:00:00Z", 10.0, 100.0, 16, None));
    let mut c = p.scan();
    {
        let st = c.state.load();
        assert_eq!(st.benchmarks.len(), 2, "two metrics from one document");
        let f = st.feature("FEAT-001").unwrap();
        assert_eq!(f.benchmarks.len(), 2, "linked by declared feature id: {:?}", st.unresolved);
        let cmp = st
            .benchmarks
            .values()
            .map(|b| b.comparison.as_ref().unwrap().classification.as_str())
            .collect::<Vec<_>>();
        assert!(cmp.iter().all(|c| *c == "unknown"), "first measurement is unknown: {cmp:?}");
        assert_eq!(perf(&c).status, RollupStatus::Unknown);
        assert_eq!(f.health.state, HealthState::Healthy, "{:?}", f.health.reasons);
        let d = st.benchmarks.values().find(|b| b.benchmark.metric.name == "commit.calls").unwrap();
        assert_eq!(d.benchmark.metric.determinism.as_str(), "deterministic");
        assert_eq!(d.benchmark.metric.determinism_origin, cockpit::time::Origin::Declared);
        assert_eq!(d.latest.as_ref().unwrap().commit.as_ref().unwrap().key.sha.as_str(), a);
    }

    // Second run at a later commit: +1 deterministic call, +40 % on the wall-clock metric.
    p.put_commit(
        "Extra index write",
        "2026-03-02T09:00:00Z",
        &[("crates/store/src/lib.rs", "fn a() { 1 }\n")],
    );
    let b = p.head_sha();
    p.put_file("perf/store.json", &doc(&b, "2026-03-02T10:00:00Z", 11.0, 140.0, 16, None));
    c.scan(None).unwrap();
    {
        let st = c.state.load();
        assert_eq!(
            c.store.count("benchmark_runs").unwrap(),
            4,
            "history keeps both runs per metric"
        );
        let calls =
            st.benchmarks.values().find(|b| b.benchmark.metric.name == "commit.calls").unwrap();
        let cmp = calls.comparison.as_ref().unwrap();
        assert_eq!(cmp.classification.as_str(), "regression");
        assert_eq!(cmp.severity, Severity::High, "deterministic regressions are loud");
        assert_eq!(cmp.baseline_value, Some(10.0));
        assert!((cmp.relative_change.unwrap() - 0.1).abs() < 1e-9);
        let origin = cmp.origin.as_ref().expect("origin");
        assert_eq!(origin.commit.as_ref().unwrap().key.sha.as_str(), b);
        assert_eq!(origin.prior_commit.as_ref().unwrap().key.sha.as_str(), a);
        assert_eq!(
            origin.suspects.iter().map(|k| k.sha.as_str()).collect::<Vec<_>>(),
            vec![b.as_str()]
        );
        let p50 = st.benchmarks.values().find(|b| b.benchmark.metric.name == "read.p50").unwrap();
        let cmp = p50.comparison.as_ref().unwrap();
        assert_eq!(cmp.classification.as_str(), "regression");
        assert_eq!(cmp.severity, Severity::Informational, "environment-sensitive never gates");
        let f = st.feature("FEAT-001").unwrap();
        assert_eq!(f.health.state, HealthState::Attention, "{:?}", f.health.reasons);
        assert_eq!(perf(&c).status, RollupStatus::Regression);
        assert_eq!(perf(&c).severity, Some(Severity::High));
        assert!(
            f.health.reasons.iter().any(
                |r| r.reason.as_str() == "benchmark_regression" && r.severity == Severity::High
            )
        );
        assert!(f.health.reasons.iter().any(|r| r.reason.as_str() == "benchmark_regression"
            && r.severity == Severity::Informational));
        assert_eq!(st.counts.benchmark_regressions, 1);
        assert_eq!(st.counts.benchmark_reported, 1);
        assert!(
            st.attention.iter().any(
                |a| a.reason.as_str() == "benchmark_regression" && a.severity == Severity::High
            )
        );
    }

    // Third run on another machine: nothing comparable, so no percentage is stated.
    p.put_file("perf/store.json", &doc(&b, "2026-03-03T10:00:00Z", 11.0, 90.0, 8, None));
    c.scan(None).unwrap();
    {
        let st = c.state.load();
        let calls =
            st.benchmarks.values().find(|b| b.benchmark.metric.name == "commit.calls").unwrap();
        let cmp = calls.comparison.as_ref().unwrap();
        assert_eq!(cmp.classification.as_str(), "incomparable", "{}", cmp.note);
        assert!(cmp.relative_change.is_none());
        assert!(cmp.note.contains("8cpu") && cmp.note.contains("16cpu"), "{}", cmp.note);
        assert_eq!(perf(&c).status, RollupStatus::Incomparable);
        let f = st.feature("FEAT-001").unwrap();
        assert_eq!(f.health.state, HealthState::Attention);
        assert!(f.health.reasons.iter().any(|r| r.reason.as_str() == "benchmark_incomparable"));
        assert_eq!(st.environments.len(), 2);
    }

    // Fourth run back on the first machine with a budget the deterministic metric exceeds.
    p.put_file("perf/store.json", &doc(&b, "2026-03-04T10:00:00Z", 12.0, 100.0, 16, Some(10.0)));
    c.scan(None).unwrap();
    {
        let st = c.state.load();
        let calls =
            st.benchmarks.values().find(|b| b.benchmark.metric.name == "commit.calls").unwrap();
        let cmp = calls.comparison.as_ref().unwrap();
        assert_eq!(cmp.classification.as_str(), "budget_breach", "{}", cmp.note);
        assert_eq!(cmp.baseline_value, Some(11.0), "baseline skips the incomparable run");
        assert_eq!(perf(&c).status, RollupStatus::BudgetBreach);
        let f = st.feature("FEAT-001").unwrap();
        assert_eq!(f.health.state, HealthState::Broken, "{:?}", f.health.reasons);
        assert_eq!(st.counts.benchmark_breaches, 1);
        assert!(st.headline.contains("1 broken"));
    }

    // Re-scanning an unchanged file is a no-op for history.
    let before = c.store.count("benchmark_runs").unwrap();
    c.scan(None).unwrap();
    assert_eq!(c.store.count("benchmark_runs").unwrap(), before);
}

#[test]
fn guardian_bench_result_maps_direction_and_dates_from_the_build_commit() {
    let p = base().config(
        "[[evidence]]\nid = \"guardian/bench\"\nformat = \"guardian-bench\"\nkind = \"benchmark\"\npath = \"perf/results/*.json\"\nfeatures = [\"FEAT-001\"]\n",
    );
    let a = p.head_sha();
    p.put_file(
        "perf/results/memory.json",
        &format!(
            r#"{{"name":"memory","generator_version":2,"shape_version":1,"workload":{{"backend":"memory","products":2000}},"build":{{"commit":"{a}","dirty":true,"rustc":"rustc 1.98.0","release":true}},"machine":{{"os":"macos","arch":"aarch64","cpus":16}},"measurements":[{{"metric":"import throughput","value":39994.4,"unit":"products/s","determinism":"environment-sensitive"}},{{"metric":"store calls: persist","value":1,"unit":"calls","determinism":"deterministic"}}],"checks":12}}"#
        ),
    );
    let c = p.scan();
    let st = c.state.load();
    assert_eq!(st.benchmarks.len(), 2);
    let thr =
        st.benchmarks.values().find(|b| b.benchmark.metric.name == "import throughput").unwrap();
    assert_eq!(thr.benchmark.metric.direction.as_str(), "higher_is_better");
    let run = thr.latest.as_ref().unwrap();
    assert_eq!(run.commit.as_ref().unwrap().key.sha.as_str(), a);
    assert_eq!(run.dirty, Some(true));
    assert_eq!(
        run.timestamp.origin,
        cockpit::time::Origin::CommitDate,
        "no generated_at → dated by the build commit"
    );
    assert_eq!(run.timestamp.value.to_string(), "2026-03-01T09:00:00Z");
    assert!(st.environments.values().next().unwrap().label.contains("memory"));
    let f = st.feature("FEAT-001").unwrap();
    assert_eq!(f.benchmarks.len(), 2, "linked through the source's `features` default");
    let cmp = thr.comparison.as_ref().unwrap();
    assert_eq!(cmp.confidence.as_str(), "low", "dirty tree lowers confidence");
}
