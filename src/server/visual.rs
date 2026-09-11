//! Visual evidence pages: the grid of checks and the compare viewer for one check.

use super::AppState;
use super::hx::{AppError, Hx, Page, fragment, render};
use super::routes::artifact_url;
use crate::model::*;
use crate::state::{ProjectState, VisualView};
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use minijinja::{Value, context};
use std::collections::{BTreeMap, HashMap};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/visual", get(visual))
        .route("/fragments/visual/grid", get(visual_grid_fragment))
        .route("/visual/{id}", get(visual_detail))
}

#[derive(Debug, Default, Clone)]
pub struct VisualFilter {
    pub status: Option<String>,
    pub group: String,
    pub feature: Option<String>,
    pub browser: Option<String>,
    pub suite: Option<String>,
    pub q: Option<String>,
    pub limit: usize,
}

impl VisualFilter {
    pub fn from_query(q: &HashMap<String, String>) -> Result<VisualFilter, AppError> {
        let opt = |k: &str| q.get(k).filter(|s| !s.is_empty() && s.as_str() != "all").cloned();
        let status = opt("status");
        if let Some(s) = &status {
            if !["regression", "missing", "stale", "inconclusive", "current", "skipped"]
                .contains(&s.as_str())
            {
                return Err(AppError::BadQuery(format!(
                    "status must be one of all, regression, missing, stale, inconclusive, current, skipped (got {s})"
                )));
            }
        }
        let group = q.get("group").cloned().unwrap_or_else(|| "suite".into());
        if !["suite", "feature", "page", "browser", "viewport", "status", "none"]
            .contains(&group.as_str())
        {
            return Err(AppError::BadQuery(format!(
                "group must be one of suite, feature, page, browser, viewport, status, none (got {group})"
            )));
        }
        Ok(VisualFilter {
            status,
            group,
            feature: opt("feature"),
            browser: opt("browser"),
            suite: opt("suite"),
            q: opt("q"),
            limit: q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(120),
        })
    }

    pub fn canonical(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for (k, v) in [
            ("status", &self.status),
            ("feature", &self.feature),
            ("browser", &self.browser),
            ("suite", &self.suite),
            ("q", &self.q),
        ] {
            if let Some(v) = v {
                parts.push(format!("{k}={}", super::view::urlencode(v)));
            }
        }
        if self.group != "suite" {
            parts.push(format!("group={}", self.group));
        }
        if self.limit != 120 {
            parts.push(format!("limit={}", self.limit));
        }
        if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) }
    }
}

fn status_rank(s: VisualStatus) -> u8 {
    match s {
        VisualStatus::Regression => 0,
        VisualStatus::Missing => 1,
        VisualStatus::Stale => 2,
        VisualStatus::Inconclusive => 3,
        VisualStatus::Current => 4,
        VisualStatus::Skipped => 5,
    }
}

fn suite_of<'a>(st: &'a ProjectState, v: &VisualView) -> Option<&'a Suite> {
    let t = v.latest.test.as_ref().and_then(|t| st.tests.get(t))?;
    st.suites.iter().find(|s| s.id == t.test.suite)
}

pub fn visual_row(st: &ProjectState, id: &crate::ids::VisualCheckId, v: &VisualView) -> Value {
    let e = &v.latest;
    let art = |a: &Option<crate::ids::ArtifactId>| a.as_ref().and_then(|a| st.artifacts.get(a));
    let baseline = art(&e.baseline);
    let actual = art(&e.actual);
    let diff = art(&e.diff);
    let url = |a: Option<&Artifact>| a.filter(|a| a.exists).map(|a| artifact_url(st, a));
    let test = e.test.as_ref().and_then(|t| st.tests.get(t));
    let suite = suite_of(st, v);
    let feats: Vec<Value> = v
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
    let diff_pct = e.metrics.difference_ratio.map(|r| r * 100.0);
    let metric_text = if let Some(p) = diff_pct {
        Some(format!("{p:.2}% differs"))
    } else if let Some(m) = e.metrics.mean_de {
        Some(format!(
            "ΔE {m:.2}{}",
            e.metrics.ssim.map(|s| format!(" · SSIM {s:.3}")).unwrap_or_default()
        ))
    } else {
        e.metrics.changed_pixels.map(|c| format!("{c} px changed"))
    };
    context! {
        id => id.as_str(),
        name => e.name.clone(),
        test_name => test.map(|t| t.test.name.clone()),
        test_id => test.map(|t| t.test.id.as_str().to_string()),
        suite => suite.map(|s| s.id.as_str().to_string()).or_else(|| test.map(|t| t.test.suite.as_str().to_string())),
        page => e.page,
        status => e.status.as_str(),
        browser => e.browser.clone(),
        viewport => e.viewport.map(|v| format!("{}×{}", v.width, v.height)),
        at => e.timestamp.value.to_string(),
        commit => e.commit.as_ref().map(|c| context! { repo => c.key.repo.as_str(), sha => c.key.sha.as_str(), short => c.key.sha.short(), resolved => c.resolved, origin => c.origin.as_str() }),
        features => feats,
        thumb => url(actual).or_else(|| url(baseline)).or_else(|| url(diff)),
        baseline_url => url(baseline),
        actual_url => url(actual),
        diff_url => url(diff),
        diff_role => diff.map(|d| d.role.as_str()),
        has_images => baseline.is_some() || actual.is_some() || diff.is_some(),
        metric_text => metric_text,
        diff_pct => diff_pct,
        metrics => Value::from_serialize(&e.metrics),
        previous_status => v.previous.as_ref().map(|p| p.status.as_str()),
        previous_at => v.previous.as_ref().map(|p| p.timestamp.value.to_string()),
    }
}

pub fn filtered_visuals<'a>(
    st: &'a ProjectState,
    f: &VisualFilter,
) -> Vec<(&'a crate::ids::VisualCheckId, &'a VisualView)> {
    let q = f.q.as_ref().map(|s| s.to_ascii_lowercase());
    let mut v: Vec<(&crate::ids::VisualCheckId, &VisualView)> = st
        .visuals
        .iter()
        .filter(|(_, v)| f.status.as_deref().map(|s| v.latest.status.as_str() == s).unwrap_or(true))
        .filter(|(_, v)| {
            f.feature
                .as_deref()
                .map(|id| v.features.iter().any(|(x, _)| x.as_str() == id))
                .unwrap_or(true)
        })
        .filter(|(_, v)| {
            f.browser.as_deref().map(|b| v.latest.browser.as_deref() == Some(b)).unwrap_or(true)
        })
        .filter(|(_, v)| {
            f.suite
                .as_deref()
                .map(|s| {
                    v.latest
                        .test
                        .as_ref()
                        .and_then(|t| st.tests.get(t))
                        .map(|t| t.test.suite.as_str() == s)
                        .unwrap_or(false)
                })
                .unwrap_or(true)
        })
        .filter(|(_, v)| {
            q.as_deref()
                .map(|q| {
                    v.latest.name.to_ascii_lowercase().contains(q)
                        || v.latest
                            .test
                            .as_ref()
                            .and_then(|t| st.tests.get(t))
                            .map(|t| t.test.name.to_ascii_lowercase().contains(q))
                            .unwrap_or(false)
                })
                .unwrap_or(true)
        })
        .collect();
    v.sort_by(|a, b| {
        status_rank(a.1.latest.status)
            .cmp(&status_rank(b.1.latest.status))
            .then_with(|| {
                let ta =
                    a.1.latest
                        .test
                        .as_ref()
                        .and_then(|t| st.tests.get(t))
                        .map(|t| t.test.name.clone())
                        .unwrap_or_default();
                let tb =
                    b.1.latest
                        .test
                        .as_ref()
                        .and_then(|t| st.tests.get(t))
                        .map(|t| t.test.name.clone())
                        .unwrap_or_default();
                ta.cmp(&tb)
            })
            .then_with(|| a.1.latest.page.cmp(&b.1.latest.page))
            .then_with(|| a.1.latest.name.cmp(&b.1.latest.name))
    });
    v
}

fn grid_ctx(st: &ProjectState, f: &VisualFilter) -> Value {
    let all = filtered_visuals(st, f);
    let total = all.len();
    let rows: Vec<(String, Value)> = all
        .iter()
        .take(f.limit)
        .map(|(id, v)| {
            let key = match f.group.as_str() {
                "feature" => v
                    .features
                    .first()
                    .map(|(x, _)| {
                        st.features
                            .get(x)
                            .map(|f| f.feature.title.clone())
                            .unwrap_or_else(|| x.to_string())
                    })
                    .unwrap_or_else(|| "No feature".into()),
                "page" => {
                    v.latest.page.map(|p| format!("Page {p}")).unwrap_or_else(|| "No page".into())
                }
                "browser" => v.latest.browser.clone().unwrap_or_else(|| "Unknown browser".into()),
                "viewport" => v
                    .latest
                    .viewport
                    .map(|vp| format!("{}×{}", vp.width, vp.height))
                    .unwrap_or_else(|| "Unknown viewport".into()),
                "status" => v.latest.status.as_str().to_string(),
                "none" => String::new(),
                _ => v
                    .latest
                    .test
                    .as_ref()
                    .and_then(|t| st.tests.get(t))
                    .map(|t| t.test.name.clone())
                    .unwrap_or_else(|| "Unknown".into()),
            };
            (key, visual_row(st, id, v))
        })
        .collect();
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    for (k, row) in rows {
        match groups.iter_mut().find(|(g, _)| g == &k) {
            Some((_, v)) => v.push(row),
            None => groups.push((k, vec![row])),
        }
    }
    let count = |s: VisualStatus| st.visuals.values().filter(|v| v.latest.status == s).count();
    let mut suites: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for v in st.visuals.values() {
        if let Some(t) = v.latest.test.as_ref().and_then(|t| st.tests.get(t)) {
            let e = suites.entry(t.test.suite.as_str().to_string()).or_default();
            e.0 += 1;
            if v.latest.status == VisualStatus::Regression {
                e.1 += 1;
            }
        }
    }
    let mut browsers: Vec<String> =
        st.visuals.values().filter_map(|v| v.latest.browser.clone()).collect();
    browsers.sort();
    browsers.dedup();
    let filtered = f.status.is_some()
        || f.feature.is_some()
        || f.browser.is_some()
        || f.suite.is_some()
        || f.q.is_some();
    context! {
        groups => groups.iter().map(|(k, v)| context! { key => k, items => v, count => v.len() }).collect::<Vec<_>>(),
        total => total,
        shown => rows_len(&groups),
        query => f.canonical(),
        filtered => filtered,
        filter => context! { status => f.status.clone(), group => f.group.clone(), feature => f.feature.clone(), browser => f.browser.clone(), suite => f.suite.clone(), q => f.q.clone() },
        counts => context! {
            all => st.visuals.len(),
            regression => count(VisualStatus::Regression),
            missing => count(VisualStatus::Missing),
            stale => count(VisualStatus::Stale),
            inconclusive => count(VisualStatus::Inconclusive),
            current => count(VisualStatus::Current),
            skipped => count(VisualStatus::Skipped),
        },
        suites => suites.iter().map(|(id, (n, r))| context! { id => id, count => n, regressions => r }).collect::<Vec<_>>(),
        browsers => browsers,
        sources => Value::from_serialize(st.sources.iter().filter(|s| s.kind == "visual" || s.format == "playwright").collect::<Vec<_>>()),
    }
}

fn rows_len(groups: &[(String, Vec<Value>)]) -> usize {
    groups.iter().map(|(_, v)| v.len()).sum()
}

async fn visual(
    hx: Hx,
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let f = VisualFilter::from_query(&q)?;
    render(
        &app,
        &hx,
        "pages/visual.html",
        "Visual",
        "visual",
        context! { grid => grid_ctx(&st, &f) },
    )
}

async fn visual_grid_fragment(
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let f = VisualFilter::from_query(&q)?;
    fragment(
        &app,
        "fragments/visual_grid.html",
        context! { grid => grid_ctx(&st, &f) },
        Some(format!("/visual{}", f.canonical())),
    )
}

pub fn visual_detail_json(
    st: &ProjectState,
    id: &crate::ids::VisualCheckId,
    v: &VisualView,
) -> serde_json::Value {
    let artifacts: Vec<serde_json::Value> = [&v.latest.baseline, &v.latest.actual, &v.latest.diff]
        .iter()
        .filter_map(|a| a.as_ref())
        .filter_map(|a| st.artifacts.get(a))
        .map(|a| serde_json::json!({ "id": a.id, "role": a.role.as_str(), "name": a.name, "url": artifact_url(st, a), "exists": a.exists, "size": a.size }))
        .collect();
    serde_json::json!({
        "check": id,
        "latest": v.latest,
        "previous": v.previous,
        "features": v.features.iter().map(|(f, o)| serde_json::json!({ "id": f, "origin": o.key(), "note": o.note() })).collect::<Vec<_>>(),
        "artifacts": artifacts,
        "links": { "html": format!("/visual/{}", super::view::urlencode(id.as_str())), "api": format!("/api/v1/visual/{}", super::view::urlencode(id.as_str())) },
    })
}

async fn visual_detail(
    hx: Hx,
    State(app): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let key = crate::ids::VisualCheckId::new(id.clone());
    let Some(v) = st.visuals.get(&key) else {
        return Err(AppError::NotFound { kind: "visual check", id, suggestions: vec![] });
    };
    let row = visual_row(&st, &key, v);
    let e = &v.latest;
    let default_mode = match e.status {
        VisualStatus::Regression if e.diff.is_some() => "diff",
        VisualStatus::Regression => "side",
        VisualStatus::Missing => "actual",
        _ => "actual",
    };
    let mode = q.get("mode").cloned().unwrap_or_else(|| default_mode.to_string());
    let test = e.test.as_ref().and_then(|t| st.tests.get(t));
    let run_row = e
        .test_run
        .as_ref()
        .and_then(|tr| test.and_then(|t| t.history.iter().find(|r| &r.id == tr)));
    // Sibling checks of the same test (other pages / snapshots) for prev/next navigation.
    let mut siblings: Vec<(&crate::ids::VisualCheckId, &VisualView)> = st
        .visuals
        .iter()
        .filter(|(_, o)| o.latest.test.is_some() && o.latest.test == e.test)
        .collect();
    siblings.sort_by(|a, b| {
        a.1.latest.page.cmp(&b.1.latest.page).then_with(|| a.1.latest.name.cmp(&b.1.latest.name))
    });
    let pos = siblings.iter().position(|(k, _)| *k == &key);
    let prev = pos
        .and_then(|p| p.checked_sub(1))
        .and_then(|p| siblings.get(p))
        .map(|(k, _)| k.as_str().to_string());
    let next = pos.and_then(|p| siblings.get(p + 1)).map(|(k, _)| k.as_str().to_string());
    let thresholds: Vec<Value> = e
        .metrics
        .thresholds
        .as_ref()
        .and_then(|t| t.as_object())
        .map(|m| m.iter().map(|(k, v)| context! { key => k, value => v.to_string() }).collect())
        .unwrap_or_default();
    let title =
        format!("{}{}", test.map(|t| format!("{} · ", t.test.name)).unwrap_or_default(), e.name);
    render(
        &app,
        &hx,
        "pages/visual_detail.html",
        &title,
        "visual",
        context! {
            v => row,
            mode => mode,
            test => test.map(|t| super::routes::test_row(&st, t)),
            run => run_row.map(Value::from_serialize),
            previous => v.previous.as_ref().map(|p| context! { status => p.status.as_str(), at => p.timestamp.value.to_string(), commit => p.commit.as_ref().map(|c| context! { repo => c.key.repo.as_str(), sha => c.key.sha.as_str(), short => c.key.sha.short() }) }),
            thresholds => thresholds,
            prev => prev,
            next => next,
            position => pos.map(|p| p + 1),
            total => siblings.len(),
            entity_id => key.as_str(),
        },
    )
}
