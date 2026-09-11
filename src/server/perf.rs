//! Performance pages: the benchmark matrix with its classification sections, and the
//! benchmark detail with history chart, regression block and environment comparison.

use super::AppState;
use super::hx::{AppError, Hx, Page, fragment, render};
use crate::model::*;
use crate::state::benchmark_cmp::{Classification, fmt_value};
use crate::state::{BenchmarkView, ProjectState};
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use minijinja::{Value, context};
use std::collections::HashMap;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/performance", get(performance))
        .route("/fragments/performance/table", get(performance_table_fragment))
        .route("/performance/{id}", get(benchmark_detail))
}

#[derive(Debug, Default, Clone)]
pub struct PerfFilter {
    pub state: Option<String>,
    pub env: Option<String>,
    pub determinism: Option<String>,
    pub feature: Option<String>,
    pub q: Option<String>,
    pub sort: String,
    pub limit: usize,
}

impl PerfFilter {
    pub fn from_query(q: &HashMap<String, String>) -> Result<PerfFilter, AppError> {
        let opt = |k: &str| q.get(k).filter(|s| !s.is_empty() && s.as_str() != "all").cloned();
        let state = opt("state");
        if let Some(s) = &state {
            if !["breach", "regression", "incomparable", "improved", "stable", "unknown"]
                .contains(&s.as_str())
            {
                return Err(AppError::BadQuery(format!(
                    "state must be one of all, breach, regression, incomparable, improved, stable, unknown (got {s})"
                )));
            }
        }
        let determinism = opt("determinism");
        if let Some(d) = &determinism {
            if Determinism::parse(d).is_none() {
                return Err(AppError::BadQuery(format!(
                    "determinism must be deterministic, semi-deterministic or environment-sensitive (got {d})"
                )));
            }
        }
        Ok(PerfFilter {
            state,
            env: opt("env"),
            determinism,
            feature: opt("feature"),
            q: opt("q"),
            sort: q.get("sort").cloned().unwrap_or_else(|| "state".into()),
            limit: q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(200),
        })
    }

    pub fn canonical(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for (k, v) in [
            ("state", &self.state),
            ("env", &self.env),
            ("determinism", &self.determinism),
            ("feature", &self.feature),
            ("q", &self.q),
        ] {
            if let Some(v) = v {
                parts.push(format!("{k}={}", super::view::urlencode(v)));
            }
        }
        if self.sort != "state" {
            parts.push(format!("sort={}", self.sort));
        }
        if self.limit != 200 {
            parts.push(format!("limit={}", self.limit));
        }
        if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) }
    }
}

fn class_key(b: &BenchmarkView) -> &'static str {
    match b.comparison.as_ref().map(|c| c.classification) {
        Some(Classification::BudgetBreach) => "breach",
        Some(Classification::Regression) => "regression",
        Some(Classification::Incomparable) => "incomparable",
        Some(Classification::Improved) => "improved",
        Some(Classification::Healthy) => "stable",
        _ => "unknown",
    }
}

fn class_rank(b: &BenchmarkView) -> u8 {
    match class_key(b) {
        "breach" => 0,
        "regression" => match b.comparison.as_ref().map(|c| c.severity) {
            Some(Severity::Informational) => 3,
            _ => 1,
        },
        "incomparable" => 2,
        "unknown" => 4,
        "stable" => 5,
        _ => 6,
    }
}

/// Rollup word for the status glyph, honouring "informational never looks like a failure".
fn status_word(b: &BenchmarkView) -> &'static str {
    match b.comparison.as_ref() {
        Some(c) => match c.classification {
            Classification::BudgetBreach => "budget_breach",
            Classification::Regression if c.severity == Severity::Informational => "reported",
            Classification::Regression => "regression",
            Classification::Incomparable => "incomparable",
            Classification::Improved => "improved",
            Classification::Healthy => "stable",
            Classification::Unknown => "first_run",
        },
        None => "silent",
    }
}

pub fn benchmark_row(st: &ProjectState, b: &BenchmarkView) -> Value {
    let unit = &b.benchmark.metric.unit;
    let c = b.comparison.as_ref();
    let feats: Vec<Value> = b
        .features
        .iter()
        .map(|(id, o)| context! {
            id => id.as_str(),
            title => st.features.get(id).map(|f| f.feature.title.clone()).unwrap_or_else(|| id.to_string()),
            explicit => o.is_explicit(),
            note => o.note(),
            state => st.features.get(id).map(|f| f.health.state.as_str()).unwrap_or("unknown"),
        })
        .collect();
    // Sparkline over the current environment class, oldest → newest.
    let series: Vec<f64> = b
        .history
        .iter()
        .filter(|r| {
            r.measurement.status == MeasurementStatus::Ok
                && b.latest.as_ref().map(|l| l.env_class == r.env_class).unwrap_or(true)
        })
        .take(30)
        .map(|r| r.measurement.value)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let budget = c.and_then(|c| c.budget).or(b.benchmark.budget);
    context! {
        id => b.benchmark.id.as_str(),
        name => b.benchmark.name.clone(),
        subject => b.benchmark.subject.clone(),
        metric => b.benchmark.metric.name.clone(),
        label => b.label(),
        unit => unit.clone(),
        value => b.latest.as_ref().map(|r| r.measurement.value),
        value_text => b.latest.as_ref().map(|r| fmt_value(r.measurement.value, unit)),
        baseline_text => c.and_then(|c| c.baseline_value).map(|v| fmt_value(v, unit)),
        change => c.and_then(|c| c.relative_change),
        change_class => c.map(|c| delta_class(c.relative_change, c.direction)).unwrap_or("mut"),
        classification => c.map(|c| c.classification.as_str()).unwrap_or("unknown"),
        class_key => class_key(b),
        class_label => c.map(|c| c.classification.label()).unwrap_or("Unknown"),
        status => status_word(b),
        severity => c.map(|c| c.severity.as_str()).unwrap_or("informational"),
        informational => c.map(|c| c.severity == Severity::Informational).unwrap_or(true),
        note => c.map(|c| c.note.clone()).unwrap_or_default(),
        determinism => b.benchmark.metric.determinism.as_str(),
        determinism_short => match b.benchmark.metric.determinism { Determinism::Deterministic => "det", Determinism::SemiDeterministic => "semi", Determinism::EnvironmentSensitive => "env" },
        direction => b.benchmark.metric.direction.as_str(),
        budget_text => budget.map(|bd| crate::state::benchmark_cmp::budget_text(Some(bd), unit)),
        env => b.environment_label.clone(),
        env_class => b.latest.as_ref().map(|r| r.env_class.as_str().to_string()),
        features => feats,
        last_run => b.latest.as_ref().map(|r| r.timestamp.value.to_string()),
        commit => b.latest.as_ref().and_then(|r| r.commit.as_ref()).map(|c| context! { repo => c.key.repo.as_str(), sha => c.key.sha.as_str(), short => c.key.sha.short() }),
        dirty => b.latest.as_ref().and_then(|r| r.dirty).unwrap_or(false),
        sparkline => sparkline_svg(&series, budget, b.benchmark.metric.direction),
        runs => b.history.len(),
    }
}

/// Colour class for a signed relative change given the metric's direction.
pub fn delta_class(rel: Option<f64>, direction: Direction) -> &'static str {
    let Some(r) = rel else { return "mut" };
    if r.abs() < 1e-9 {
        return "mut";
    }
    match direction {
        Direction::LowerIsBetter => {
            if r > 0.0 {
                "bad"
            } else {
                "ok"
            }
        }
        Direction::HigherIsBetter => {
            if r < 0.0 {
                "bad"
            } else {
                "ok"
            }
        }
        Direction::Informational => "mut",
    }
}

/// Inline 64×16 sparkline; the last point is emphasised, the budget drawn as a dashed line.
pub fn sparkline_svg(values: &[f64], budget: Option<Budget>, direction: Direction) -> Value {
    if values.len() < 2 {
        return Value::from_safe_string(String::new());
    }
    let w = 64.0;
    let h = 16.0;
    let mut lo = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let mut hi = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let budget_v = match budget {
        Some(Budget::Max(m)) | Some(Budget::Min(m)) => Some(m),
        _ => None,
    };
    if let Some(bv) = budget_v {
        lo = lo.min(bv);
        hi = hi.max(bv);
    }
    if (hi - lo).abs() < 1e-12 {
        hi = lo + 1.0;
        lo -= 1.0;
    }
    let x = |i: usize| 1.0 + (w - 2.0) * i as f64 / (values.len() - 1) as f64;
    let y = |v: f64| 1.0 + (h - 2.0) * (1.0 - (v - lo) / (hi - lo));
    let pts: Vec<String> =
        values.iter().enumerate().map(|(i, v)| format!("{:.1},{:.1}", x(i), y(*v))).collect();
    let last = values[values.len() - 1];
    let prev = values[values.len() - 2];
    let cls =
        delta_class(if prev != 0.0 { Some((last - prev) / prev.abs()) } else { None }, direction);
    let mut svg = format!(
        "<svg class=\"spark\" width=\"64\" height=\"16\" viewBox=\"0 0 64 16\" role=\"img\" aria-label=\"{} values, latest {}\">",
        values.len(),
        last
    );
    if let Some(bv) = budget_v {
        svg.push_str(&format!(
            "<line class=\"spark__budget\" x1=\"0\" x2=\"64\" y1=\"{:.1}\" y2=\"{:.1}\"/>",
            y(bv),
            y(bv)
        ));
    }
    svg.push_str(&format!("<polyline class=\"spark__line\" points=\"{}\"/>", pts.join(" ")));
    svg.push_str(&format!(
        "<circle class=\"spark__dot spark__dot--{cls}\" cx=\"{:.1}\" cy=\"{:.1}\" r=\"2\"/>",
        x(values.len() - 1),
        y(last)
    ));
    svg.push_str("</svg>");
    Value::from_safe_string(svg)
}

pub fn filtered_benchmarks<'a>(st: &'a ProjectState, f: &PerfFilter) -> Vec<&'a BenchmarkView> {
    let q = f.q.as_ref().map(|s| s.to_ascii_lowercase());
    let mut v: Vec<&BenchmarkView> = st
        .benchmarks
        .values()
        .filter(|b| f.state.as_deref().map(|s| class_key(b) == s).unwrap_or(true))
        .filter(|b| {
            f.env
                .as_deref()
                .map(|e| b.latest.as_ref().map(|r| r.env_class.as_str() == e).unwrap_or(false))
                .unwrap_or(true)
        })
        .filter(|b| {
            f.determinism
                .as_deref()
                .and_then(Determinism::parse)
                .map(|d| b.benchmark.metric.determinism == d)
                .unwrap_or(true)
        })
        .filter(|b| {
            f.feature
                .as_deref()
                .map(|id| b.features.iter().any(|(x, _)| x.as_str() == id))
                .unwrap_or(true)
        })
        .filter(|b| {
            q.as_deref()
                .map(|q| {
                    b.label().to_ascii_lowercase().contains(q)
                        || b.environment_label.to_ascii_lowercase().contains(q)
                })
                .unwrap_or(true)
        })
        .collect();
    match f.sort.as_str() {
        "name" => v.sort_by_key(|a| a.label()),
        "change" => v.sort_by(|a, b| {
            let ka = a
                .comparison
                .as_ref()
                .and_then(|c| c.relative_change)
                .map(|r| r.abs())
                .unwrap_or(-1.0);
            let kb = b
                .comparison
                .as_ref()
                .and_then(|c| c.relative_change)
                .map(|r| r.abs())
                .unwrap_or(-1.0);
            kb.partial_cmp(&ka)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.label().cmp(&b.label()))
        }),
        "recent" => v.sort_by(|a, b| {
            let ka = a.latest.as_ref().map(|r| r.timestamp.value);
            let kb = b.latest.as_ref().map(|r| r.timestamp.value);
            kb.cmp(&ka).then_with(|| a.label().cmp(&b.label()))
        }),
        _ => v.sort_by(|a, b| {
            class_rank(a).cmp(&class_rank(b)).then_with(|| a.label().cmp(&b.label()))
        }),
    }
    v
}

fn table_ctx(st: &ProjectState, f: &PerfFilter) -> Value {
    let all = filtered_benchmarks(st, f);
    let total = all.len();
    let rows: Vec<Value> = all.iter().take(f.limit).map(|b| benchmark_row(st, b)).collect();
    let count = |k: &str| st.benchmarks.values().filter(|b| class_key(b) == k).count();
    let reported = st
        .benchmarks
        .values()
        .filter(|b| {
            class_key(b) == "regression"
                && b.comparison
                    .as_ref()
                    .map(|c| c.severity == Severity::Informational)
                    .unwrap_or(false)
        })
        .count();
    // Environment classes present, for the filter.
    let mut envs: Vec<(String, String, usize)> = Vec::new();
    for b in st.benchmarks.values() {
        if let Some(r) = &b.latest {
            match envs.iter_mut().find(|(id, _, _)| id == r.env_class.as_str()) {
                Some(e) => e.2 += 1,
                None => {
                    envs.push((r.env_class.as_str().to_string(), b.environment_label.clone(), 1))
                }
            }
        }
    }
    envs.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.cmp(&b.1)));
    let breaches: Vec<Value> = st
        .benchmarks
        .values()
        .filter(|b| class_key(b) == "breach")
        .map(|b| benchmark_row(st, b))
        .collect();
    let mut regressions: Vec<&BenchmarkView> = st
        .benchmarks
        .values()
        .filter(|b| {
            class_key(b) == "regression"
                && b.comparison
                    .as_ref()
                    .map(|c| c.severity != Severity::Informational)
                    .unwrap_or(false)
        })
        .collect();
    regressions.sort_by(|a, b| {
        let ka =
            a.comparison.as_ref().and_then(|c| c.relative_change).map(|r| r.abs()).unwrap_or(0.0);
        let kb =
            b.comparison.as_ref().and_then(|c| c.relative_change).map(|r| r.abs()).unwrap_or(0.0);
        kb.partial_cmp(&ka).unwrap_or(std::cmp::Ordering::Equal)
    });
    let hbar: Vec<serde_json::Value> = regressions
        .iter()
        .take(10)
        .filter_map(|b| {
            let c = b.comparison.as_ref()?;
            Some(serde_json::json!({ "label": b.label(), "pct": c.relative_change.map(|r| r * 100.0), "id": b.benchmark.id.as_str(), "worse": true }))
        })
        .collect();
    let regressions: Vec<Value> = regressions.iter().map(|b| benchmark_row(st, b)).collect();
    let incomparable: Vec<Value> = st
        .benchmarks
        .values()
        .filter(|b| class_key(b) == "incomparable")
        .map(|b| benchmark_row(st, b))
        .collect();
    let improved: Vec<Value> = st
        .benchmarks
        .values()
        .filter(|b| class_key(b) == "improved")
        .map(|b| benchmark_row(st, b))
        .collect();
    let filtered = f.state.is_some()
        || f.env.is_some()
        || f.determinism.is_some()
        || f.feature.is_some()
        || f.q.is_some();
    context! {
        rows => rows,
        total => total,
        shown => rows.len(),
        query => f.canonical(),
        filtered => filtered,
        filter => context! { state => f.state.clone(), env => f.env.clone(), determinism => f.determinism.clone(), feature => f.feature.clone(), q => f.q.clone(), sort => f.sort.clone() },
        counts => context! {
            all => st.benchmarks.len(),
            breach => count("breach"),
            regression => count("regression") - reported,
            reported => reported,
            incomparable => count("incomparable"),
            improved => count("improved"),
            stable => count("stable"),
            unknown => count("unknown"),
        },
        envs => envs.iter().map(|(id, label, n)| context! { id => id, label => label, count => n }).collect::<Vec<_>>(),
        breaches => breaches,
        regressions => regressions,
        hbar => Value::from_serialize(&hbar),
        incomparable => incomparable,
        improved => improved,
        sources => Value::from_serialize(st.sources.iter().filter(|s| s.kind == "benchmark" || s.format.contains("bench") || s.format.contains("measure") || s.format == "criterion").collect::<Vec<_>>()),
    }
}

async fn performance(
    hx: Hx,
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let f = PerfFilter::from_query(&q)?;
    render(
        &app,
        &hx,
        "pages/performance.html",
        "Performance",
        "performance",
        context! { table => table_ctx(&st, &f) },
    )
}

async fn performance_table_fragment(
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let f = PerfFilter::from_query(&q)?;
    fragment(
        &app,
        "fragments/performance_table.html",
        context! { table => table_ctx(&st, &f) },
        Some(format!("/performance{}", f.canonical())),
    )
}

/// Everything the detail page and the JSON API say about one benchmark.
pub fn benchmark_detail_json(
    st: &ProjectState,
    b: &BenchmarkView,
    range: &str,
) -> serde_json::Value {
    let unit = &b.benchmark.metric.unit;
    let now = crate::time::now();
    let cutoff = match range {
        "30d" => Some(now - jiff::SignedDuration::from_secs(30 * 86_400)),
        "90d" => Some(now - jiff::SignedDuration::from_secs(90 * 86_400)),
        "1y" => Some(now - jiff::SignedDuration::from_secs(365 * 86_400)),
        _ => None,
    };
    let current_class = b.latest.as_ref().map(|r| r.env_class.clone());
    let mut points: Vec<serde_json::Value> = b
        .history
        .iter()
        .filter(|r| r.measurement.status == MeasurementStatus::Ok)
        .filter(|r| cutoff.map(|c| r.timestamp.value >= c).unwrap_or(true))
        .map(|r| {
            let same = current_class.as_ref().map(|c| c == &r.env_class).unwrap_or(true);
            serde_json::json!({
                "run": r.id, "t": r.timestamp.value.to_string(), "ms": crate::time::millis(r.timestamp.value),
                "v": r.measurement.value, "text": fmt_value(r.measurement.value, unit),
                "sha": r.commit.as_ref().map(|c| c.key.sha.short().to_string()),
                "sha_full": r.commit.as_ref().map(|c| c.key.sha.as_str().to_string()),
                "repo": r.commit.as_ref().map(|c| c.key.repo.as_str().to_string()),
                "branch": r.branch, "dirty": r.dirty, "env": st.environments.get(&r.environment).map(|e| e.label.clone()),
                "comparable": same, "p50": r.measurement.statistics.as_ref().and_then(|s| s.p50), "p95": r.measurement.statistics.as_ref().and_then(|s| s.p95),
                "min": r.measurement.statistics.as_ref().and_then(|s| s.min), "max": r.measurement.statistics.as_ref().and_then(|s| s.max),
                "accepted": r.measurement.accepted,
            })
        })
        .collect();
    points.reverse();
    if points.len() > 2000 {
        let step = points.len() as f64 / 2000.0;
        let mut kept = Vec::new();
        let mut i = 0.0;
        while (i as usize) < points.len() {
            kept.push(points[i as usize].clone());
            i += step;
        }
        points = kept;
    }
    let tags: Vec<serde_json::Value> = st
        .tags
        .iter()
        .filter_map(|(repo, tag, sha)| {
            let at = st.commits.get(&crate::ids::CommitKey { repo: repo.clone(), sha: sha.clone() })?.commit.committed_at;
            Some(serde_json::json!({ "tag": tag, "sha": sha.short(), "ms": crate::time::millis(at) }))
        })
        .collect();
    let c = b.comparison.as_ref();
    let budget = c.and_then(|c| c.budget).or(b.benchmark.budget);
    let budget_value = match budget {
        Some(Budget::Max(m)) | Some(Budget::Min(m)) => Some(m),
        _ => None,
    };
    let env_of = |id: &crate::ids::EnvironmentId| {
        st.environments.get(id).map(|e| serde_json::to_value(e).unwrap_or_default())
    };
    serde_json::json!({
        "benchmark": b.benchmark,
        "label": b.label(),
        "features": b.features.iter().map(|(id, o)| serde_json::json!({ "id": id, "title": st.features.get(id).map(|f| f.feature.title.clone()), "state": st.features.get(id).map(|f| f.health.state.as_str()), "origin": o.key(), "note": o.note() })).collect::<Vec<_>>(),
        "latest": b.latest,
        "comparison": c,
        "classification": c.map(|c| c.classification.as_str()),
        "status": status_word(b),
        "current_env": b.latest.as_ref().and_then(|r| env_of(&r.environment)),
        "baseline_env": c.and_then(|c| c.baseline_env.as_ref()).and_then(env_of),
        "chart": { "points": points, "tags": tags, "budget": budget_value, "baseline": c.and_then(|c| c.baseline_value), "baseline_ms": c.and_then(|c| c.baseline_at).map(crate::time::millis), "origin_ms": c.and_then(|c| c.origin.as_ref()).map(|o| crate::time::millis(o.at)), "unit": unit, "direction": b.benchmark.metric.direction.as_str(), "range": range },
        "history_total": b.history.len(),
        "links": { "html": format!("/performance/{}", super::view::urlencode(b.benchmark.id.as_str())), "api": format!("/api/v1/benchmarks/{}", super::view::urlencode(b.benchmark.id.as_str())) },
    })
}

async fn benchmark_detail(
    hx: Hx,
    State(app): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let Some(b) = st.benchmarks.get(&crate::ids::BenchmarkId::new(id.clone())) else {
        let suggestions: Vec<(String, String)> = st
            .benchmarks
            .values()
            .filter(|b| b.label().to_ascii_lowercase().contains(&id.to_ascii_lowercase()))
            .take(5)
            .map(|b| {
                (
                    format!("/performance/{}", super::view::urlencode(b.benchmark.id.as_str())),
                    b.label(),
                )
            })
            .collect();
        return Err(AppError::NotFound { kind: "benchmark", id, suggestions });
    };
    let tab = q.get("tab").cloned().unwrap_or_else(|| "history".into());
    let range = q.get("range").cloned().unwrap_or_else(|| "all".into());
    let detail = benchmark_detail_json(&st, b, &range);
    let unit = &b.benchmark.metric.unit;
    let c = b.comparison.as_ref();
    let history_rows: Vec<Value> = b
        .history
        .iter()
        .take(100)
        .map(|r| context! {
            id => r.id.as_str(),
            at => r.timestamp.value.to_string(),
            at_origin => r.timestamp.origin.as_str(),
            value => r.measurement.value,
            value_text => fmt_value(r.measurement.value, unit),
            status => r.measurement.status.as_str(),
            reason => r.measurement.reason.clone(),
            commit => r.commit.as_ref().map(|cr| context! { repo => cr.key.repo.as_str(), sha => cr.key.sha.as_str(), short => cr.key.sha.short(), origin => cr.origin.as_str(), resolved => cr.resolved }),
            branch => r.branch.clone(),
            dirty => r.dirty.unwrap_or(false),
            env => st.environments.get(&r.environment).map(|e| e.label.clone()),
            comparable => b.latest.as_ref().map(|l| l.env_class == r.env_class).unwrap_or(true),
            is_baseline => c.and_then(|c| c.baseline_run.as_ref()).map(|x| x == &r.id).unwrap_or(false),
            is_current => b.latest.as_ref().map(|l| l.id == r.id).unwrap_or(false),
            is_origin => c.and_then(|c| c.origin.as_ref()).map(|o| o.run == r.id).unwrap_or(false),
            accepted => r.measurement.accepted.clone(),
            stats => r.measurement.statistics.as_ref().map(Value::from_serialize),
            samples => r.measurement.samples.as_ref().map(|s| s.len()).unwrap_or(0),
        })
        .collect();
    // Environment diff: current vs baseline fingerprint, differing rows flagged.
    let env_rows = env_diff(&st, b);
    let suspects: Vec<Value> = c
        .and_then(|c| c.origin.as_ref())
        .map(|o| {
            o.suspects
                .iter()
                .filter_map(|k| st.commits.get(k))
                .map(|cv| context! { repo => cv.commit.key.repo.as_str(), sha => cv.commit.key.sha.as_str(), short => cv.commit.key.sha.short(), subject => cv.commit.subject.clone(), at => cv.commit.committed_at.to_string(), author => cv.commit.author.clone(), files => cv.commit.files.len() })
                .collect()
        })
        .unwrap_or_default();
    let origin = c.and_then(|c| c.origin.as_ref()).map(|o| context! {
        run => o.run.as_str(),
        at => o.at.to_string(),
        value_text => fmt_value(o.value, unit),
        prior_value_text => o.prior_value.map(|v| fmt_value(v, unit)),
        commit => o.commit.as_ref().map(|cr| context! { repo => cr.key.repo.as_str(), sha => cr.key.sha.as_str(), short => cr.key.sha.short(), subject => st.commits.get(&cr.key).map(|x| x.commit.subject.clone()) }),
        prior_commit => o.prior_commit.as_ref().map(|cr| context! { repo => cr.key.repo.as_str(), sha => cr.key.sha.as_str(), short => cr.key.sha.short() }),
        suspects => suspects,
    });
    let samples: Vec<f64> =
        b.latest.as_ref().and_then(|r| r.measurement.samples.clone()).unwrap_or_default();
    let dist = distribution(&samples);
    render(
        &app,
        &hx,
        "pages/benchmark_detail.html",
        &b.label(),
        "performance",
        context! {
            b => benchmark_row(&st, b),
            detail => Value::from_serialize(&detail),
            chart_json => serde_json::to_string(&detail["chart"]).unwrap_or_else(|_| "{}".into()),
            comparison => c.map(Value::from_serialize),
            baseline => c.map(|c| context! {
                mode => c.baseline_mode.clone(),
                value_text => c.baseline_value.map(|v| fmt_value(v, unit)),
                at => c.baseline_at.map(|t| t.to_string()),
                commit => c.baseline_commit.as_ref().map(|cr| context! { repo => cr.key.repo.as_str(), sha => cr.key.sha.as_str(), short => cr.key.sha.short() }),
                env => c.baseline_env.as_ref().and_then(|e| st.environments.get(e)).map(|e| e.label.clone()),
                noise_pct => c.noise_pct,
                gate => c.gate,
                significance => c.significance.clone(),
                accepted => c.accepted.clone(),
                confidence => c.confidence.as_str(),
            }),
            origin => origin,
            history => history_rows,
            env_rows => env_rows,
            current_env => b.latest.as_ref().and_then(|r| st.environments.get(&r.environment)).map(Value::from_serialize),
            tab => tab,
            range => range,
            samples_n => samples.len(),
            dist => dist,
            raw_json => serde_json::to_string_pretty(&serde_json::json!({ "benchmark": b.benchmark, "latest": b.latest, "comparison": c })).unwrap_or_default(),
            entity_id => b.benchmark.id.as_str(),
        },
    )
}

fn env_diff(st: &ProjectState, b: &BenchmarkView) -> Vec<Value> {
    let cur = b.latest.as_ref().and_then(|r| st.environments.get(&r.environment));
    let base = b
        .comparison
        .as_ref()
        .and_then(|c| c.baseline_env.as_ref())
        .and_then(|e| st.environments.get(e));
    let Some(cur) = cur else { return vec![] };
    let field = |e: &EnvironmentFingerprint, k: &str| -> String {
        match k {
            "os" => e.os.clone().unwrap_or_default(),
            "arch" => e.arch.clone().unwrap_or_default(),
            "cpus" => e.cpus.map(|c| c.to_string()).unwrap_or_default(),
            "cpu_model" => e.cpu_model.clone().unwrap_or_default(),
            "runner" => e.runner.clone().unwrap_or_default(),
            "profile" => e.profile.clone().unwrap_or_default(),
            "compiler" => e.compiler.clone().unwrap_or_default(),
            "tool" => e
                .tool
                .as_ref()
                .map(|t| {
                    format!(
                        "{}{}",
                        t.name,
                        t.version.as_ref().map(|v| format!(" {v}")).unwrap_or_default()
                    )
                })
                .unwrap_or_default(),
            _ => String::new(),
        }
    };
    let mut rows: Vec<Value> = Vec::new();
    for k in ["os", "arch", "cpus", "cpu_model", "runner", "profile", "compiler", "tool"] {
        let a = field(cur, k);
        let bv = base.map(|e| field(e, k));
        if a.is_empty() && bv.as_deref().map(|s| s.is_empty()).unwrap_or(true) {
            continue;
        }
        let differs = bv.as_ref().map(|x| x != &a).unwrap_or(false);
        let key = cur.comparability_keys.iter().any(|c| c.ends_with(k));
        rows.push(context! { key => k, current => a, baseline => bv, differs => differs, comparability => key });
    }
    // Workload keys.
    if let Some(obj) = cur.workload.as_object() {
        for (k, v) in obj {
            let a = compact(v);
            let bv = base.and_then(|e| e.workload.get(k)).map(compact);
            let differs = bv.as_ref().map(|x| x != &a).unwrap_or(false);
            rows.push(context! { key => format!("workload.{k}"), current => a, baseline => bv, differs => differs, comparability => true });
        }
    }
    if let Some(obj) = cur.extra.as_object() {
        for (k, v) in obj {
            let a = compact(v);
            let bv = base.and_then(|e| e.extra.get(k)).map(compact);
            let differs = bv.as_ref().map(|x| x != &a).unwrap_or(false);
            rows.push(context! { key => k, current => a, baseline => bv, differs => differs, comparability => false });
        }
    }
    rows
}

fn compact(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => {
            let s = other.to_string();
            if s.chars().count() > 80 {
                format!("{}…", s.chars().take(79).collect::<String>())
            } else {
                s
            }
        }
    }
}

fn distribution(samples: &[f64]) -> Option<Value> {
    if samples.is_empty() {
        return None;
    }
    let mut s: Vec<f64> = samples.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q = |p: f64| s[((s.len() - 1) as f64 * p).round() as usize];
    let mean = s.iter().sum::<f64>() / s.len() as f64;
    let var = s.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / s.len() as f64;
    // 24-bucket histogram for a pure-HTML strip.
    let lo = s[0];
    let hi = s[s.len() - 1];
    let buckets = 24usize;
    let mut hist = vec![0usize; buckets];
    for x in &s {
        let i = if hi > lo {
            (((x - lo) / (hi - lo)) * (buckets as f64 - 1.0)).floor() as usize
        } else {
            0
        };
        hist[i.min(buckets - 1)] += 1;
    }
    let max = *hist.iter().max().unwrap_or(&1) as f64;
    Some(context! {
        n => s.len(), min => lo, max => hi, mean => mean, std_dev => var.sqrt(), p50 => q(0.5), p95 => q(0.95), p99 => q(0.99),
        hist => hist.iter().map(|h| (*h as f64 / max * 100.0).round() as i64).collect::<Vec<_>>(),
    })
}

/// Overview "Performance changes": breaches, regressions, incomparable, then improvements.
pub fn overview_changes(st: &ProjectState, limit: usize) -> Vec<Value> {
    let mut v: Vec<&BenchmarkView> = st
        .benchmarks
        .values()
        .filter(|b| matches!(class_key(b), "breach" | "regression" | "incomparable" | "improved"))
        .collect();
    v.sort_by(|a, b| {
        class_rank(a).cmp(&class_rank(b)).then_with(|| {
            let ka = a
                .comparison
                .as_ref()
                .and_then(|c| c.relative_change)
                .map(|r| r.abs())
                .unwrap_or(0.0);
            let kb = b
                .comparison
                .as_ref()
                .and_then(|c| c.relative_change)
                .map(|r| r.abs())
                .unwrap_or(0.0);
            kb.partial_cmp(&ka).unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    v.into_iter().take(limit).map(|b| benchmark_row(st, b)).collect()
}
