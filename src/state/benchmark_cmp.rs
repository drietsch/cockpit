//! Benchmark comparison and classification (§36, ADR-0057 lessons): a result is a document,
//! comparisons happen only between comparable environments, and the class of the metric decides
//! how loudly a change may speak.

use crate::config::{PerformanceSection, RegressionPolicy};
use crate::git::CommitGraph;
use crate::ids::*;
use crate::model::*;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Healthy,
    Improved,
    Regression,
    BudgetBreach,
    Unknown,
    Incomparable,
}

impl Classification {
    pub fn as_str(&self) -> &'static str {
        match self {
            Classification::Healthy => "healthy",
            Classification::Improved => "improved",
            Classification::Regression => "regression",
            Classification::BudgetBreach => "budget_breach",
            Classification::Unknown => "unknown",
            Classification::Incomparable => "incomparable",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Classification::Healthy => "Stable",
            Classification::Improved => "Improved",
            Classification::Regression => "Regression",
            Classification::BudgetBreach => "Budget breach",
            Classification::Unknown => "Unknown",
            Classification::Incomparable => "Incomparable",
        }
    }
    pub fn rollup(&self) -> RollupStatus {
        match self {
            Classification::Healthy => RollupStatus::Pass,
            Classification::Improved => RollupStatus::Improved,
            Classification::Regression => RollupStatus::Regression,
            Classification::BudgetBreach => RollupStatus::BudgetBreach,
            Classification::Unknown => RollupStatus::Unknown,
            Classification::Incomparable => RollupStatus::Incomparable,
        }
    }
}

/// Where a regression started: the run after which the value stayed worse.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegressionOrigin {
    pub run: BenchmarkRunId,
    pub commit: Option<CommitRef>,
    pub at: Timestamp,
    pub value: f64,
    pub prior_run: Option<BenchmarkRunId>,
    pub prior_commit: Option<CommitRef>,
    pub prior_value: Option<f64>,
    /// Commits between prior and origin (same repo), most suspicious first.
    pub suspects: Vec<CommitKey>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkComparison {
    pub classification: Classification,
    /// Severity the classification carries into feature health; `Informational` never gates.
    pub severity: Severity,
    pub determinism: Determinism,
    pub direction: Direction,
    pub current_run: BenchmarkRunId,
    pub current_value: f64,
    pub current_env: EnvironmentId,
    pub current_env_class: EnvClassId,
    pub baseline_run: Option<BenchmarkRunId>,
    pub baseline_value: Option<f64>,
    pub baseline_commit: Option<CommitRef>,
    pub baseline_at: Option<Timestamp>,
    pub baseline_env: Option<EnvironmentId>,
    /// `previous-comparable`, `tag:v1.2`, `commit:abc123` — what the baseline was chosen by.
    pub baseline_mode: String,
    pub absolute_change: Option<f64>,
    /// Fraction, direction-agnostic sign (positive = value went up).
    pub relative_change: Option<f64>,
    pub noise_pct: f64,
    pub gate: bool,
    pub budget: Option<Budget>,
    pub budget_breached: bool,
    pub significance: Option<String>,
    pub accepted: Option<String>,
    pub confidence: Confidence,
    pub note: String,
    pub origin: Option<RegressionOrigin>,
}

pub struct CompareInput<'a> {
    pub benchmark: &'a Benchmark,
    /// Every known run for the benchmark, any order.
    pub runs: &'a [BenchmarkRun],
    pub cfg: &'a PerformanceSection,
    pub environments: &'a HashMap<EnvironmentId, EnvironmentFingerprint>,
    pub tags: &'a [(RepoId, String, Sha)],
    pub graphs: &'a HashMap<RepoId, CommitGraph>,
    /// Paths owned by the features this benchmark is linked to (for ranking suspects).
    pub owned_paths: &'a globset::GlobSet,
    pub commit_files: &'a HashMap<CommitKey, Vec<String>>,
}

fn policy(cfg: &PerformanceSection, d: Determinism) -> &RegressionPolicy {
    match d {
        Determinism::Deterministic => &cfg.policy.deterministic,
        Determinism::SemiDeterministic => &cfg.policy.semi_deterministic,
        Determinism::EnvironmentSensitive => &cfg.policy.environment_sensitive,
    }
}

/// Is `value` worse than `base` beyond `noise_pct` in the metric's direction?
fn worse_beyond(value: f64, base: f64, direction: Direction, noise_pct: f64) -> bool {
    if base == 0.0 {
        return match direction {
            Direction::LowerIsBetter => value > 0.0 && noise_pct <= 0.0,
            _ => false,
        };
    }
    let rel = (value - base) / base.abs() * 100.0;
    match direction {
        Direction::LowerIsBetter => rel > noise_pct,
        Direction::HigherIsBetter => rel < -noise_pct,
        Direction::Informational => false,
    }
}

fn better_beyond(value: f64, base: f64, direction: Direction, noise_pct: f64) -> bool {
    match direction {
        Direction::LowerIsBetter => worse_beyond(value, base, Direction::HigherIsBetter, noise_pct),
        Direction::HigherIsBetter => worse_beyond(value, base, Direction::LowerIsBetter, noise_pct),
        Direction::Informational => false,
    }
}

fn breaches(value: f64, budget: Option<Budget>, rel: Option<f64>, direction: Direction) -> bool {
    match budget {
        Some(Budget::Max(m)) => value > m,
        Some(Budget::Min(m)) => value < m,
        Some(Budget::MaxRelativePct(p)) => match (rel, direction) {
            (Some(r), Direction::LowerIsBetter) => r * 100.0 > p,
            (Some(r), Direction::HigherIsBetter) => r * 100.0 < -p,
            _ => false,
        },
        None => false,
    }
}

pub fn compare(input: &CompareInput) -> Option<BenchmarkComparison> {
    let b = input.benchmark;
    let mut runs: Vec<&BenchmarkRun> =
        input.runs.iter().filter(|r| r.measurement.status == MeasurementStatus::Ok).collect();
    if runs.is_empty() {
        return None;
    }
    // Newest first; ties by id for determinism across scan order.
    runs.sort_by(|a, c| c.timestamp.value.cmp(&a.timestamp.value).then_with(|| c.id.cmp(&a.id)));
    let current = runs[0];
    let determinism = b.metric.determinism;
    let direction = b.metric.direction;
    let pol = policy(input.cfg, determinism);
    let noise_pct = pol.noise_pct;
    let env_label = |id: &EnvironmentId| {
        input
            .environments
            .get(id)
            .map(|e| e.label.clone())
            .unwrap_or_else(|| "unknown environment".into())
    };
    let budget = current.budget.or(b.budget);
    let base_conf = if current.dirty == Some(true) { Confidence::Low } else { Confidence::High };
    let mut cmp = BenchmarkComparison {
        classification: Classification::Unknown,
        severity: Severity::Informational,
        determinism,
        direction,
        current_run: current.id.clone(),
        current_value: current.measurement.value,
        current_env: current.environment.clone(),
        current_env_class: current.env_class.clone(),
        baseline_run: None,
        baseline_value: None,
        baseline_commit: None,
        baseline_at: None,
        baseline_env: None,
        baseline_mode: input.cfg.baseline.clone(),
        absolute_change: None,
        relative_change: None,
        noise_pct,
        gate: pol.gate,
        budget,
        budget_breached: false,
        significance: current.measurement.reported_change.as_ref().map(|c| match c.significant {
            Some(true) => {
                format!("producer reports a significant change of {:+.1}%", c.relative * 100.0)
            }
            Some(false) => format!("producer reports {:+.1}% within noise", c.relative * 100.0),
            None => format!("producer reports {:+.1}%", c.relative * 100.0),
        }),
        accepted: current.measurement.accepted.clone(),
        confidence: base_conf,
        note: String::new(),
        origin: None,
    };

    // Baseline selection.
    let earlier: Vec<&BenchmarkRun> = runs.iter().copied().skip(1).collect();
    let comparable: Vec<&BenchmarkRun> =
        earlier.iter().copied().filter(|r| r.env_class == current.env_class).collect();
    let mode = input.cfg.baseline.trim();
    let mut baseline: Option<&BenchmarkRun> = None;
    let mut mode_note = String::new();
    if let Some(tag) = mode.strip_prefix("tag:") {
        let shas: Vec<&Sha> =
            input.tags.iter().filter(|(_, t, _)| t == tag).map(|(_, _, s)| s).collect();
        baseline = comparable
            .iter()
            .copied()
            .find(|r| r.commit.as_ref().map(|c| shas.contains(&&c.key.sha)).unwrap_or(false));
        if baseline.is_none() {
            mode_note =
                format!("no comparable run at tag {tag}; using the previous comparable run");
        }
    } else if let Some(sha) = mode.strip_prefix("commit:") {
        let p = sha.to_ascii_lowercase();
        baseline = comparable.iter().copied().find(|r| {
            r.commit.as_ref().map(|c| c.key.sha.as_str().starts_with(&p)).unwrap_or(false)
        });
        if baseline.is_none() {
            mode_note =
                format!("no comparable run at commit {sha}; using the previous comparable run");
        }
    }
    if baseline.is_none() {
        if !mode_note.is_empty() {
            cmp.note = mode_note.clone();
            cmp.baseline_mode = format!("{mode} → previous-comparable");
        }
        // Previous comparable: prefer the same branch and a clean tree, then anything in the class.
        baseline = comparable
            .iter()
            .copied()
            .find(|r| r.branch == current.branch && r.dirty != Some(true))
            .or_else(|| comparable.iter().copied().find(|r| r.dirty != Some(true)))
            .or_else(|| comparable.first().copied());
    }

    let Some(base) = baseline else {
        if earlier.is_empty() {
            cmp.classification = Classification::Unknown;
            cmp.note = "first measurement — nothing to compare against yet".into();
        } else {
            let other = &earlier[0];
            cmp.classification = Classification::Incomparable;
            cmp.baseline_run = Some(other.id.clone());
            cmp.baseline_value = Some(other.measurement.value);
            cmp.baseline_commit = other.commit.clone();
            cmp.baseline_at = Some(other.timestamp.value);
            cmp.baseline_env = Some(other.environment.clone());
            cmp.severity = Severity::Low;
            cmp.note = format!(
                "latest run on {}, previous history on {} — environments differ, so no change is stated",
                env_label(&current.environment),
                env_label(&other.environment)
            );
        }
        cmp.budget_breached = breaches(current.measurement.value, budget, None, direction);
        if cmp.budget_breached {
            cmp.classification = Classification::BudgetBreach;
            cmp.severity = if determinism == Determinism::EnvironmentSensitive {
                Severity::Medium
            } else {
                Severity::High
            };
            cmp.note = format!(
                "{} exceeds its budget ({})",
                fmt_value(current.measurement.value, &b.metric.unit),
                budget_text(budget, &b.metric.unit)
            );
        }
        return Some(cmp);
    };

    cmp.baseline_run = Some(base.id.clone());
    cmp.baseline_value = Some(base.measurement.value);
    cmp.baseline_commit = base.commit.clone();
    cmp.baseline_at = Some(base.timestamp.value);
    cmp.baseline_env = Some(base.environment.clone());
    let cur = current.measurement.value;
    let bv = base.measurement.value;
    let abs = cur - bv;
    let rel = if bv != 0.0 { Some(abs / bv.abs()) } else { None };
    cmp.absolute_change = Some(abs);
    cmp.relative_change = rel;
    if base.dirty == Some(true) {
        cmp.confidence = Confidence::Low;
    }
    if base.commit.as_ref().map(|c| !c.resolved).unwrap_or(true)
        || current.commit.as_ref().map(|c| !c.resolved).unwrap_or(true)
    {
        cmp.confidence = cmp.confidence.min(Confidence::Medium);
    }

    cmp.budget_breached = breaches(cur, budget, rel, direction);
    let worse = worse_beyond(cur, bv, direction, noise_pct);
    let better = better_beyond(cur, bv, direction, noise_pct);
    let change_text = match rel {
        Some(r) => format!("{:+.1}%", r * 100.0),
        None => format!("{:+}", abs),
    };
    let vs = format!(
        "{} vs {} ({change_text})",
        fmt_value(cur, &b.metric.unit),
        fmt_value(bv, &b.metric.unit)
    );
    if cmp.budget_breached {
        cmp.classification = Classification::BudgetBreach;
        cmp.severity = if determinism == Determinism::EnvironmentSensitive {
            Severity::Medium
        } else {
            Severity::High
        };
        cmp.note = format!(
            "{} exceeds its budget ({})",
            fmt_value(cur, &b.metric.unit),
            budget_text(budget, &b.metric.unit)
        );
    } else if direction == Direction::Informational {
        cmp.classification = Classification::Healthy;
        cmp.note = format!("{vs} — informational metric, no direction is better");
    } else if worse {
        cmp.classification = Classification::Regression;
        let significant = current.measurement.reported_change.as_ref().and_then(|c| c.significant);
        cmp.severity = match determinism {
            Determinism::Deterministic => Severity::High,
            Determinism::SemiDeterministic => Severity::Medium,
            Determinism::EnvironmentSensitive => {
                if pol.gate && significant == Some(true) {
                    Severity::Medium
                } else {
                    Severity::Informational
                }
            }
        };
        if pol.require_significance && significant != Some(true) {
            cmp.severity = Severity::Informational;
        }
        cmp.note = match determinism {
            Determinism::Deterministic => {
                format!("{vs} — deterministic metric, any change is real")
            }
            Determinism::SemiDeterministic => {
                format!("{vs} — beyond the {noise_pct:.0}% noise band")
            }
            Determinism::EnvironmentSensitive => {
                format!("{vs} — environment-sensitive, reported but never gating")
            }
        };
        cmp.origin = origin(&runs, current, base, direction, noise_pct, input);
    } else if better {
        cmp.classification = Classification::Improved;
        cmp.note = format!("{vs} — improved");
    } else {
        cmp.classification = Classification::Healthy;
        cmp.note = if noise_pct > 0.0 {
            format!("{vs} — within the {noise_pct:.0}% noise band")
        } else {
            format!("{vs} — unchanged")
        };
    }
    if let Some(a) = &cmp.accepted {
        if matches!(cmp.classification, Classification::Regression) {
            cmp.severity = Severity::Informational;
            cmp.note = format!("{} — accepted: {a}", cmp.note);
        }
    }
    Some(cmp)
}

/// The first run after the baseline from which the value stayed worse than the baseline.
/// With a `previous-comparable` baseline this is the current run itself; with a pinned
/// baseline (`tag:` / `commit:`) it locates the step change.
fn origin(
    runs: &[&BenchmarkRun],
    current: &BenchmarkRun,
    base: &BenchmarkRun,
    direction: Direction,
    noise_pct: f64,
    input: &CompareInput,
) -> Option<RegressionOrigin> {
    // Newest-first, same class, strictly between the baseline (exclusive) and now (inclusive).
    let series: Vec<&BenchmarkRun> = runs
        .iter()
        .copied()
        .filter(|r| r.env_class == current.env_class && r.timestamp.value > base.timestamp.value)
        .collect();
    let mut k = 0usize;
    while k < series.len()
        && worse_beyond(series[k].measurement.value, base.measurement.value, direction, noise_pct)
    {
        k += 1;
    }
    if k == 0 {
        return None;
    }
    let origin_run = series[k - 1];
    let prior = series.get(k).copied().unwrap_or(base);
    let mut suspects: Vec<CommitKey> = Vec::new();
    if let (Some(oc), Some(pc)) = (&origin_run.commit, &prior.commit) {
        if oc.key.repo == pc.key.repo && oc.resolved && pc.resolved {
            if let Some(g) = input.graphs.get(&oc.key.repo) {
                let shas = g.between(&pc.key.sha, &oc.key.sha, 500);
                let mut ranked: Vec<(i32, CommitKey)> = shas
                    .into_iter()
                    .map(|s| {
                        let key = CommitKey { repo: oc.key.repo.clone(), sha: s };
                        let touches = input
                            .commit_files
                            .get(&key)
                            .map(|fs| {
                                fs.iter().filter(|f| input.owned_paths.is_match(f.as_str())).count()
                            })
                            .unwrap_or(0);
                        (touches as i32, key)
                    })
                    .collect();
                ranked.sort_by_key(|(touches, _)| std::cmp::Reverse(*touches));
                suspects = ranked.into_iter().take(20).map(|(_, k)| k).collect();
            }
        }
    }
    Some(RegressionOrigin {
        run: origin_run.id.clone(),
        commit: origin_run.commit.clone(),
        at: origin_run.timestamp.value,
        value: origin_run.measurement.value,
        prior_run: Some(prior.id.clone()),
        prior_commit: prior.commit.clone(),
        prior_value: Some(prior.measurement.value),
        suspects,
    })
}

pub fn budget_text(b: Option<Budget>, unit: &str) -> String {
    match b {
        Some(Budget::Max(m)) => format!("max {}", fmt_value(m, unit)),
        Some(Budget::Min(m)) => format!("min {}", fmt_value(m, unit)),
        Some(Budget::MaxRelativePct(p)) => format!("max +{p:.0}% vs baseline"),
        None => "none".into(),
    }
}

/// Unit-aware, three significant digits: `1.23 ms`, `48.1 MB/s`, `12 calls`.
pub fn fmt_value(v: f64, unit: &str) -> String {
    let u = unit.trim();
    let lu = u.to_ascii_lowercase();
    let (scaled, su): (f64, String) = match lu.as_str() {
        "ns" => scale_time(v / 1e9),
        "us" | "µs" => scale_time(v / 1e6),
        "ms" => scale_time(v / 1e3),
        "s" | "sec" | "seconds" => scale_time(v),
        "bytes" | "b" => scale_bytes(v),
        _ => (v, u.to_string()),
    };
    let num = if scaled == scaled.trunc() && scaled.abs() < 1e6 {
        format!("{}", scaled as i64)
    } else if scaled.abs() >= 100.0 {
        format!("{scaled:.0}")
    } else if scaled.abs() >= 10.0 {
        format!("{scaled:.1}")
    } else {
        format!("{scaled:.2}")
    };
    if su.is_empty() { num } else { format!("{num} {su}") }
}

fn scale_time(secs: f64) -> (f64, String) {
    let a = secs.abs();
    if a >= 1.0 {
        (secs, "s".into())
    } else if a >= 1e-3 {
        (secs * 1e3, "ms".into())
    } else if a >= 1e-6 {
        (secs * 1e6, "µs".into())
    } else {
        (secs * 1e9, "ns".into())
    }
}

fn scale_bytes(b: f64) -> (f64, String) {
    let a = b.abs();
    if a >= 1024.0 * 1024.0 * 1024.0 {
        (b / (1024.0 * 1024.0 * 1024.0), "GiB".into())
    } else if a >= 1024.0 * 1024.0 {
        (b / (1024.0 * 1024.0), "MiB".into())
    } else if a >= 1024.0 {
        (b / 1024.0, "KiB".into())
    } else {
        (b, "B".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{Origin, Stamped};

    fn bench(det: Determinism, dir: Direction, budget: Option<Budget>) -> Benchmark {
        Benchmark {
            id: BenchmarkId::new("B-1"),
            source: SourceId::new("s"),
            name: "memory".into(),
            subject: None,
            metric: MetricDef {
                name: "commit.calls".into(),
                unit: "calls".into(),
                direction: dir,
                direction_origin: Origin::Declared,
                determinism: det,
                determinism_origin: Origin::Declared,
            },
            budget,
            component: None,
            declared_feature_ids: vec![],
            first_seen: crate::time::parse_timestamp("2026-01-01T00:00:00Z").unwrap(),
            last_seen: crate::time::parse_timestamp("2026-01-01T00:00:00Z").unwrap(),
        }
    }

    fn run(id: &str, value: f64, day: u8, class: &str) -> BenchmarkRun {
        BenchmarkRun {
            id: BenchmarkRunId::new(format!("BR-{id}")),
            run: RunId::new(format!("R-{id}")),
            benchmark: BenchmarkId::new("B-1"),
            measurement: Measurement {
                value,
                unit: "calls".into(),
                status: MeasurementStatus::Ok,
                reason: None,
                statistics: None,
                samples: None,
                accepted: None,
                reported_change: None,
            },
            commit: Some(CommitRef {
                key: CommitKey {
                    repo: RepoId::new("."),
                    sha: Sha::new(format!("{id}{id}{id}{id}")),
                },
                origin: CommitOrigin::Declared,
                resolved: true,
            }),
            branch: Some("main".into()),
            dirty: Some(false),
            timestamp: Stamped::new(
                crate::time::parse_timestamp(&format!("2026-01-{day:02}T00:00:00Z")).unwrap(),
                Origin::Declared,
            ),
            environment: EnvironmentId::new(format!("E-{class}")),
            env_class: EnvClassId::new(format!("EC-{class}")),
            budget: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn input<'a>(
        b: &'a Benchmark,
        runs: &'a [BenchmarkRun],
        cfg: &'a PerformanceSection,
        envs: &'a HashMap<EnvironmentId, EnvironmentFingerprint>,
        tags: &'a [(RepoId, String, Sha)],
        graphs: &'a HashMap<RepoId, CommitGraph>,
        gs: &'a globset::GlobSet,
        cf: &'a HashMap<CommitKey, Vec<String>>,
    ) -> CompareInput<'a> {
        CompareInput {
            benchmark: b,
            runs,
            cfg,
            environments: envs,
            tags,
            graphs,
            owned_paths: gs,
            commit_files: cf,
        }
    }

    #[test]
    fn deterministic_change_is_a_regression_and_env_sensitive_is_informational() {
        let cfg = PerformanceSection::default();
        let envs = HashMap::new();
        let tags = vec![];
        let graphs = HashMap::new();
        let gs = globset::GlobSet::empty();
        let cf = HashMap::new();
        let runs = vec![run("a", 10.0, 1, "x"), run("b", 11.0, 2, "x")];
        let b = bench(Determinism::Deterministic, Direction::LowerIsBetter, None);
        let c = compare(&input(&b, &runs, &cfg, &envs, &tags, &graphs, &gs, &cf)).unwrap();
        assert_eq!(c.classification, Classification::Regression);
        assert_eq!(c.severity, Severity::High);
        assert!(c.origin.is_some());
        let b = bench(Determinism::EnvironmentSensitive, Direction::LowerIsBetter, None);
        let runs = vec![run("a", 10.0, 1, "x"), run("b", 14.0, 2, "x")];
        let c = compare(&input(&b, &runs, &cfg, &envs, &tags, &graphs, &gs, &cf)).unwrap();
        assert_eq!(c.classification, Classification::Regression);
        assert_eq!(c.severity, Severity::Informational);
        let runs = vec![run("a", 10.0, 1, "x"), run("b", 11.0, 2, "x")];
        let c = compare(&input(&b, &runs, &cfg, &envs, &tags, &graphs, &gs, &cf)).unwrap();
        assert_eq!(c.classification, Classification::Healthy);
    }

    #[test]
    fn other_environment_is_incomparable_and_budgets_gate() {
        let cfg = PerformanceSection::default();
        let envs = HashMap::new();
        let tags = vec![];
        let graphs = HashMap::new();
        let gs = globset::GlobSet::empty();
        let cf = HashMap::new();
        let b = bench(Determinism::Deterministic, Direction::LowerIsBetter, None);
        let runs = vec![run("a", 10.0, 1, "x"), run("b", 20.0, 2, "y")];
        let c = compare(&input(&b, &runs, &cfg, &envs, &tags, &graphs, &gs, &cf)).unwrap();
        assert_eq!(c.classification, Classification::Incomparable);
        assert!(c.relative_change.is_none());
        let b = bench(
            Determinism::SemiDeterministic,
            Direction::LowerIsBetter,
            Some(Budget::Max(15.0)),
        );
        let c = compare(&input(&b, &runs, &cfg, &envs, &tags, &graphs, &gs, &cf)).unwrap();
        assert_eq!(c.classification, Classification::BudgetBreach);
        assert_eq!(c.severity, Severity::High);
        let b = bench(Determinism::Deterministic, Direction::HigherIsBetter, None);
        let runs = vec![run("a", 10.0, 1, "x"), run("b", 12.0, 2, "x")];
        let c = compare(&input(&b, &runs, &cfg, &envs, &tags, &graphs, &gs, &cf)).unwrap();
        assert_eq!(c.classification, Classification::Improved);
        let runs = vec![run("a", 10.0, 1, "x")];
        let c = compare(&input(&b, &runs, &cfg, &envs, &tags, &graphs, &gs, &cf)).unwrap();
        assert_eq!(c.classification, Classification::Unknown);
    }

    #[test]
    fn origin_walks_back_over_a_sustained_regression() {
        let cfg = PerformanceSection { baseline: "commit:bbbb".into(), ..Default::default() };
        let envs = HashMap::new();
        let tags = vec![];
        let graphs = HashMap::new();
        let gs = globset::GlobSet::empty();
        let cf = HashMap::new();
        let b = bench(Determinism::Deterministic, Direction::LowerIsBetter, None);
        // 10, 10, 12, 12, 13: origin is the first 12 (day 3).
        let runs = vec![
            run("a", 10.0, 1, "x"),
            run("b", 10.0, 2, "x"),
            run("c", 12.0, 3, "x"),
            run("d", 12.0, 4, "x"),
            run("e", 13.0, 5, "x"),
        ];
        let c = compare(&input(&b, &runs, &cfg, &envs, &tags, &graphs, &gs, &cf)).unwrap();
        assert_eq!(c.classification, Classification::Regression);
        assert_eq!(c.baseline_run.as_ref().unwrap().as_str(), "BR-b");
        assert_eq!(c.origin.as_ref().unwrap().run.as_str(), "BR-c");
        assert_eq!(c.origin.as_ref().unwrap().prior_run.as_ref().unwrap().as_str(), "BR-b");
        // Previous-comparable: the origin is the current run.
        let cfg = PerformanceSection::default();
        let c = compare(&input(&b, &runs, &cfg, &envs, &tags, &graphs, &gs, &cf)).unwrap();
        assert_eq!(c.origin.as_ref().unwrap().run.as_str(), "BR-e");
    }

    #[test]
    fn values_format_with_units() {
        assert_eq!(fmt_value(1500.0, "us"), "1.50 ms");
        assert_eq!(fmt_value(12.0, "calls"), "12 calls");
        assert_eq!(fmt_value(2048.0, "bytes"), "2 KiB");
        assert_eq!(fmt_value(48.123, "MB/s"), "48.1 MB/s");
    }
}
