//! HTML pages and fragments. Each handler builds a view from the snapshot and renders it.

use super::AppState;
use super::hx::{AppError, Hx, Page, fragment, render};
use crate::ids::{AdrId, FeatureId};
use crate::model::*;
use crate::state::{FeatureView, ProjectState};
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use minijinja::{Value, context};
use std::collections::HashMap;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(overview))
        .route("/attention", get(attention))
        .route("/features", get(features))
        .route("/fragments/features/table", get(features_table_fragment))
        .route("/features/{id}", get(feature_detail))
        .route("/changes", get(changes))
        .route("/commits/{sha}", get(commit))
        .route("/adrs", get(adrs))
        .route("/adrs/{id}", get(adr_detail))
        .route("/status", get(status))
        .route("/search", get(search))
        .route("/tests", get(tests))
        .route("/tests/{id}", get(test_detail))
        .route("/fragments/tests/table", get(tests_table_fragment))
        .route("/performance", get(coming_soon))
        .route("/visual", get(coming_soon))
}

pub async fn not_found(hx: Hx, State(app): State<AppState>) -> Result<Page, AppError> {
    let mut p = render(
        &app,
        &hx,
        "pages/error.html",
        "Not found",
        "",
        context! { title => "Not found", message => "There is no page at this address." },
    )?;
    p.status = axum::http::StatusCode::NOT_FOUND;
    Ok(p)
}

async fn coming_soon(
    hx: Hx,
    State(app): State<AppState>,
    uri: axum::http::Uri,
) -> Result<Page, AppError> {
    let nav = uri.path().trim_start_matches('/').to_string();
    render(
        &app,
        &hx,
        "pages/coming_soon.html",
        "Coming soon",
        &nav,
        context! { section => nav.clone() },
    )
}

// ── Overview ────────────────────────────────────────────────────────────────

async fn overview(hx: Hx, State(app): State<AppState>) -> Result<Page, AppError> {
    let st = app.state.load();
    if st.generation == 0 {
        return render(&app, &hx, "pages/scanning.html", "Scanning", "overview", context! {});
    }
    let attention: Vec<Value> =
        st.attention.iter().take(8).map(|a| attention_row(&st, a)).collect();
    let changes = change_rows(&st, None, None, 8);
    let matrix: Vec<Value> =
        st.features_sorted().into_iter().take(15).map(|v| feature_row(&st, v)).collect();
    render(
        &app,
        &hx,
        "pages/overview.html",
        "Overview",
        "overview",
        context! {
            attention => attention,
            changes => changes,
            matrix => matrix,
            sources => Value::from_serialize(&st.sources),
            features_path => st.project.root.display().to_string(),
        },
    )
}

pub fn attention_row(st: &ProjectState, a: &AttentionItem) -> Value {
    let fv = a.feature.as_ref().and_then(|f| st.features.get(f));
    let state = fv.map(|f| f.health.state.as_str()).unwrap_or("attention");
    let strip: Vec<Value> = fv
        .map(|f| {
            f.health
                .rollups
                .iter()
                .map(|r| context! { label => r.category.label(), status => r.status.as_str() })
                .collect()
        })
        .unwrap_or_default();
    context! {
        id => a.id.as_str(),
        state => state,
        severity => a.severity.as_str(),
        feature_id => a.feature.as_ref().map(|f| f.as_str().to_string()),
        feature_title => a.feature_title.clone(),
        title => a.title.clone(),
        summary => a.summary.clone(),
        why => a.why_it_matters.clone(),
        first_seen => a.first_seen.to_string(),
        commit => a.commit.as_ref().map(|c| context! { repo => c.repo.as_str(), sha => c.sha.as_str(), short => c.sha.short() }),
        strip => strip,
        subject => a.subject.clone(),
        reason => a.reason.as_str(),
    }
}

pub fn change_rows(
    st: &ProjectState,
    feature: Option<&str>,
    repo: Option<&str>,
    limit: usize,
) -> Vec<Value> {
    let mut out = Vec::new();
    for k in &st.recent_commits {
        if out.len() >= limit {
            break;
        }
        let Some(c) = st.commits.get(k) else { continue };
        if let Some(r) = repo {
            if c.commit.key.repo.as_str() != r {
                continue;
            }
        }
        if let Some(f) = feature {
            if !c.features.iter().any(|(id, _)| id.as_str() == f) {
                continue;
            }
        }
        out.push(commit_row(st, c));
    }
    out
}

pub fn commit_row(st: &ProjectState, c: &crate::state::CommitView) -> Value {
    let feats: Vec<Value> = c
        .features
        .iter()
        .map(|(id, o)| {
            let title = st.features.get(id).map(|f| f.feature.title.clone()).unwrap_or_else(|| id.to_string());
            let state = st.features.get(id).map(|f| f.health.state.as_str()).unwrap_or("unknown");
            context! { id => id.as_str(), title => title, explicit => o.is_explicit(), note => o.note(), state => state }
        })
        .collect();
    let added: u32 = c.commit.files.iter().filter_map(|f| f.added).sum();
    let deleted: u32 = c.commit.files.iter().filter_map(|f| f.deleted).sum();
    context! {
        repo => c.commit.key.repo.as_str(),
        sha => c.commit.key.sha.as_str(),
        short => c.commit.short(),
        subject => c.commit.subject.clone(),
        author => c.commit.author.clone(),
        at => c.commit.committed_at.to_string(),
        features => feats,
        adr_refs => c.adr_refs.iter().map(|a| a.as_str().to_string()).collect::<Vec<_>>(),
        files => c.commit.files.len(),
        added => added,
        deleted => deleted,
        tags => c.commit.tags(),
    }
}

pub fn feature_row(_st: &ProjectState, v: &FeatureView) -> Value {
    let f = &v.feature;
    let rollup = |cat: EvidenceCategory| {
        v.health
            .rollups
            .iter()
            .find(|r| r.category == cat)
            .map(|r| r.status.as_str().to_string())
            .unwrap_or_else(|| "missing".into())
    };
    context! {
        id => f.id.as_str(),
        title => f.title.clone(),
        state => v.health.state.as_str(),
        state_label => v.health.state.label(),
        headline => v.health.headline.clone(),
        importance => f.importance.as_str(),
        lifecycle => f.lifecycle.as_str(),
        group => f.group.join("/"),
        tags => f.tags.clone(),
        owners => f.owners.clone(),
        unit => rollup(EvidenceCategory::Unit),
        integration => rollup(EvidenceCategory::Integration),
        e2e => rollup(EvidenceCategory::E2e),
        visual => rollup(EvidenceCategory::Visual),
        perf => rollup(EvidenceCategory::Perf),
        adrs => v.adrs.len(),
        changed_at => v.impl_committed_at.map(|t| t.to_string()),
        changed_sha => v.impl_commit.as_ref().map(|c| c.sha.short().to_string()),
        changed_repo => v.impl_commit.as_ref().map(|c| c.repo.as_str().to_string()),
        reasons => v.health.reasons.len(),
        confidence => v.health.confidence.as_str(),
        claims => f.claims.iter().map(|c| context! { component => c.component.as_str(), status => c.status.as_str(), note => c.note.clone() }).collect::<Vec<_>>(),
    }
}

// ── Features ────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct FeatureFilter {
    pub state: Vec<String>,
    pub importance: Vec<String>,
    pub tag: Option<String>,
    pub group: Option<String>,
    pub owner: Option<String>,
    pub q: Option<String>,
    pub sort: String,
    pub limit: usize,
}

impl FeatureFilter {
    pub fn from_query(q: &HashMap<String, String>) -> FeatureFilter {
        let list = |k: &str| {
            q.get(k)
                .map(|s| {
                    s.split(',')
                        .map(|x| x.trim().to_string())
                        .filter(|x| !x.is_empty() && x != "all")
                        .collect()
                })
                .unwrap_or_default()
        };
        FeatureFilter {
            state: list("state"),
            importance: list("importance"),
            tag: q.get("tag").filter(|s| !s.is_empty()).cloned(),
            group: q.get("group").filter(|s| !s.is_empty()).cloned(),
            owner: q.get("owner").filter(|s| !s.is_empty()).cloned(),
            q: q.get("q").filter(|s| !s.is_empty()).cloned(),
            sort: q.get("sort").cloned().unwrap_or_else(|| "rank".into()),
            limit: q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(100),
        }
    }

    pub fn canonical(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.state.is_empty() {
            parts.push(format!("state={}", self.state.join(",")));
        }
        if !self.importance.is_empty() {
            parts.push(format!("importance={}", self.importance.join(",")));
        }
        for (k, v) in
            [("tag", &self.tag), ("group", &self.group), ("owner", &self.owner), ("q", &self.q)]
        {
            if let Some(v) = v {
                parts.push(format!("{k}={}", super::view::urlencode(v)));
            }
        }
        if self.sort != "rank" {
            parts.push(format!("sort={}", self.sort));
        }
        if self.limit != 100 {
            parts.push(format!("limit={}", self.limit));
        }
        if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) }
    }
}

pub fn filtered_features<'a>(st: &'a ProjectState, f: &FeatureFilter) -> Vec<&'a FeatureView> {
    let q = f.q.as_ref().map(|s| s.to_ascii_lowercase());
    let mut rows: Vec<&FeatureView> = st
        .features
        .values()
        .filter(|v| f.state.is_empty() || f.state.iter().any(|s| s == v.health.state.as_str()))
        .filter(|v| {
            f.importance.is_empty()
                || f.importance.iter().any(|s| s == v.feature.importance.as_str())
        })
        .filter(|v| f.tag.as_ref().map(|t| v.feature.tags.contains(t)).unwrap_or(true))
        .filter(|v| {
            f.group
                .as_ref()
                .map(|g| v.feature.group.join("/") == *g || v.feature.group.first() == Some(g))
                .unwrap_or(true)
        })
        .filter(|v| f.owner.as_ref().map(|o| v.feature.owners.contains(o)).unwrap_or(true))
        .filter(|v| match &q {
            Some(q) => {
                v.feature.id.as_str().to_ascii_lowercase().contains(q)
                    || v.feature.title.to_ascii_lowercase().contains(q)
            }
            None => true,
        })
        .collect();
    match f.sort.as_str() {
        "name" => rows.sort_by(|a, b| a.feature.title.cmp(&b.feature.title)),
        "id" => rows.sort_by(|a, b| a.feature.id.cmp(&b.feature.id)),
        "changed" => rows.sort_by(|a, b| {
            b.impl_committed_at
                .cmp(&a.impl_committed_at)
                .then_with(|| a.feature.id.cmp(&b.feature.id))
        }),
        "importance" => rows.sort_by(|a, b| {
            a.feature
                .importance
                .cmp(&b.feature.importance)
                .then_with(|| b.health.rank_score.cmp(&a.health.rank_score))
        }),
        _ => rows.sort_by(|a, b| {
            b.health
                .rank_score
                .cmp(&a.health.rank_score)
                .then_with(|| a.feature.id.cmp(&b.feature.id))
        }),
    }
    rows
}

fn feature_table_ctx(st: &ProjectState, f: &FeatureFilter) -> Value {
    let all = filtered_features(st, f);
    let total = all.len();
    let rows: Vec<Value> = all.iter().take(f.limit).map(|v| feature_row(st, v)).collect();
    let mut groups: Vec<String> =
        st.features.values().filter_map(|v| v.feature.group.first().cloned()).collect();
    groups.sort();
    groups.dedup();
    let mut tags: Vec<String> = st.features.values().flat_map(|v| v.feature.tags.clone()).collect();
    tags.sort();
    tags.dedup();
    let mut owners: Vec<String> =
        st.features.values().flat_map(|v| v.feature.owners.clone()).collect();
    owners.sort();
    owners.dedup();
    let has_claims = st.features.values().any(|v| !v.feature.claims.is_empty());
    context! {
        rows => rows,
        total => total,
        shown => rows.len(),
        filter => context! { state => f.state.clone(), importance => f.importance.clone(), tag => f.tag.clone(), group => f.group.clone(), owner => f.owner.clone(), q => f.q.clone(), sort => f.sort.clone(), limit => f.limit },
        query => f.canonical(),
        groups => groups,
        tags => tags,
        owners => owners,
        has_claims => has_claims,
        components => Value::from_serialize(&st.project.components),
        counts => Value::from_serialize(&st.counts),
    }
}

async fn features(
    hx: Hx,
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let f = FeatureFilter::from_query(&q);
    render(
        &app,
        &hx,
        "pages/features.html",
        "Features",
        "features",
        context! { table => feature_table_ctx(&st, &f) },
    )
}

async fn features_table_fragment(
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let f = FeatureFilter::from_query(&q);
    fragment(
        &app,
        "fragments/features_table.html",
        context! { table => feature_table_ctx(&st, &f) },
        Some(format!("/features{}", f.canonical())),
    )
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct TestFilter {
    pub tab: String,
    pub kind: Option<String>,
    pub feature: Option<String>,
    pub suite: Option<String>,
    pub q: Option<String>,
    pub sort: String,
    pub limit: usize,
}

impl TestFilter {
    pub fn from_query(q: &HashMap<String, String>) -> TestFilter {
        TestFilter {
            tab: q.get("status").cloned().filter(|s| !s.is_empty()).unwrap_or_else(|| "all".into()),
            kind: q.get("kind").filter(|s| !s.is_empty()).cloned(),
            feature: q.get("feature").filter(|s| !s.is_empty()).cloned(),
            suite: q.get("suite").filter(|s| !s.is_empty()).cloned(),
            q: q.get("q").filter(|s| !s.is_empty()).cloned(),
            sort: q.get("sort").cloned().unwrap_or_else(|| "status".into()),
            limit: q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(200),
        }
    }
    pub fn canonical(&self) -> String {
        let mut parts = Vec::new();
        if self.tab != "all" {
            parts.push(format!("status={}", self.tab));
        }
        for (k, v) in [
            ("kind", &self.kind),
            ("feature", &self.feature),
            ("suite", &self.suite),
            ("q", &self.q),
        ] {
            if let Some(v) = v {
                parts.push(format!("{k}={}", super::view::urlencode(v)));
            }
        }
        if self.sort != "status" {
            parts.push(format!("sort={}", self.sort));
        }
        if self.limit != 200 {
            parts.push(format!("limit={}", self.limit));
        }
        if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) }
    }
}

fn status_rank(t: &crate::state::TestView) -> u8 {
    match t.latest.as_ref().map(|r| r.status) {
        Some(TestStatus::Failed) => 0,
        _ if t.flaky => 1,
        Some(TestStatus::Flaky) => 1,
        Some(TestStatus::Skipped) => 3,
        None => 4,
        _ => 2,
    }
}

pub fn test_row(st: &ProjectState, t: &crate::state::TestView) -> Value {
    let status = if t.flaky
        && t.latest.as_ref().map(|r| r.status != TestStatus::Failed).unwrap_or(false)
    {
        "flaky".to_string()
    } else {
        t.latest.as_ref().map(|r| r.status.as_str().to_string()).unwrap_or_else(|| "unknown".into())
    };
    let feats: Vec<Value> = t
        .features
        .iter()
        .map(|(id, o)| context! { id => id.as_str(), title => st.features.get(id).map(|f| f.feature.title.clone()).unwrap_or_else(|| id.to_string()), explicit => o.is_explicit(), note => o.note(), state => st.features.get(id).map(|f| f.health.state.as_str()).unwrap_or("unknown") })
        .collect();
    let strip: Vec<&str> = t.history.iter().take(10).map(|r| r.status.as_str()).collect();
    let suite_name = st
        .suites
        .iter()
        .find(|s| s.id == t.test.suite)
        .map(|s| s.id.as_str().to_string())
        .unwrap_or_else(|| t.test.suite.to_string());
    context! {
        id => t.test.id.as_str(),
        name => t.test.name.clone(),
        classname => t.test.classname.clone(),
        status => status,
        kind => t.test.kind.as_str(),
        suite => suite_name,
        features => feats,
        duration_ms => t.latest.as_ref().and_then(|r| r.duration_ms),
        last_run => t.latest.as_ref().map(|r| r.timestamp.value.to_string()),
        strip => strip,
        flaky => t.flaky,
        flaky_text => t.flaky_text.clone(),
        source_path => t.test.source_path.as_ref().map(|p| p.as_str().to_string()),
        message => t.latest.as_ref().and_then(|r| r.message.clone()),
    }
}

pub fn filtered_tests<'a>(st: &'a ProjectState, f: &TestFilter) -> Vec<&'a crate::state::TestView> {
    let q = f.q.as_ref().map(|s| s.to_ascii_lowercase());
    let mut rows: Vec<&crate::state::TestView> = st
        .tests
        .values()
        .filter(|t| match f.tab.as_str() {
            "failing" => t.latest.as_ref().map(|r| r.status == TestStatus::Failed).unwrap_or(false),
            "flaky" => {
                t.flaky || t.latest.as_ref().map(|r| r.status == TestStatus::Flaky).unwrap_or(false)
            }
            "slow" => {
                t.latest.as_ref().and_then(|r| r.duration_ms).map(|d| d >= 1000).unwrap_or(false)
            }
            "recent" => t
                .latest
                .as_ref()
                .map(|r| crate::time::now().duration_since(r.timestamp.value).as_secs() < 86_400)
                .unwrap_or(false),
            "unlinked" => t.features.is_empty(),
            _ => true,
        })
        .filter(|t| f.kind.as_ref().map(|k| t.test.kind.as_str() == k).unwrap_or(true))
        .filter(|t| {
            f.feature
                .as_ref()
                .map(|fid| {
                    if fid == "none" {
                        t.features.is_empty()
                    } else {
                        t.features.iter().any(|(id, _)| id.as_str() == fid)
                    }
                })
                .unwrap_or(true)
        })
        .filter(|t| f.suite.as_ref().map(|s| t.test.suite.as_str() == s).unwrap_or(true))
        .filter(|t| match &q {
            Some(q) => {
                t.test.name.to_ascii_lowercase().contains(q)
                    || t.test.key.to_ascii_lowercase().contains(q)
            }
            None => true,
        })
        .collect();
    match f.sort.as_str() {
        "duration" => rows.sort_by(|a, b| {
            b.latest
                .as_ref()
                .and_then(|r| r.duration_ms)
                .cmp(&a.latest.as_ref().and_then(|r| r.duration_ms))
        }),
        "recent" => rows.sort_by(|a, b| {
            b.latest
                .as_ref()
                .map(|r| r.timestamp.value)
                .cmp(&a.latest.as_ref().map(|r| r.timestamp.value))
        }),
        "name" => rows.sort_by(|a, b| a.test.name.cmp(&b.test.name)),
        _ => rows.sort_by(|a, b| {
            status_rank(a).cmp(&status_rank(b)).then_with(|| a.test.name.cmp(&b.test.name))
        }),
    }
    rows
}

fn tests_table_ctx(st: &ProjectState, f: &TestFilter) -> Value {
    let all = filtered_tests(st, f);
    let total = all.len();
    let rows: Vec<Value> = all.iter().take(f.limit).map(|t| test_row(st, t)).collect();
    let failing = st
        .tests
        .values()
        .filter(|t| t.latest.as_ref().map(|r| r.status == TestStatus::Failed).unwrap_or(false))
        .count();
    let flaky = st
        .tests
        .values()
        .filter(|t| {
            t.flaky || t.latest.as_ref().map(|r| r.status == TestStatus::Flaky).unwrap_or(false)
        })
        .count();
    let unlinked = st.tests.values().filter(|t| t.features.is_empty()).count();
    let suites: Vec<Value> = st
        .suites
        .iter()
        .map(|s| {
            let silent = st.attention.iter().find(|a| a.reason == AttentionReason::SuiteSilent && a.subject == s.id.0).map(|a| a.summary.clone());
            let n = st.tests.values().filter(|t| t.test.suite == s.id).count();
            context! { id => s.id.as_str(), name => s.name.clone(), kind => s.default_kind.as_str(), tests => n, last_seen => s.last_seen.map(|t| t.to_string()), runs => s.run_count, silent => silent, component => s.component.as_ref().map(|c| c.as_str().to_string()) }
        })
        .collect();
    context! {
        rows => rows, total => total, shown => rows.len(),
        filter => context! { tab => f.tab.clone(), kind => f.kind.clone(), feature => f.feature.clone(), suite => f.suite.clone(), q => f.q.clone(), sort => f.sort.clone() },
        query => f.canonical(),
        counts => context! { all => st.tests.len(), failing => failing, flaky => flaky, unlinked => unlinked },
        suites => suites,
        sources => Value::from_serialize(&st.sources),
    }
}

async fn tests(
    hx: Hx,
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let f = TestFilter::from_query(&q);
    render(
        &app,
        &hx,
        "pages/tests.html",
        "Tests",
        "tests",
        context! { table => tests_table_ctx(&st, &f) },
    )
}

async fn tests_table_fragment(
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let f = TestFilter::from_query(&q);
    fragment(
        &app,
        "fragments/tests_table.html",
        context! { table => tests_table_ctx(&st, &f) },
        Some(format!("/tests{}", f.canonical())),
    )
}

async fn test_detail(
    hx: Hx,
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let Some(t) = st.tests.get(&crate::ids::TestId::new(id.clone())) else {
        return Err(AppError::NotFound { kind: "test", id, suggestions: vec![] });
    };
    let history: Vec<Value> = t
        .history
        .iter()
        .map(|r| context! { id => r.id.as_str(), status => r.status.as_str(), at => r.timestamp.value.to_string(), duration_ms => r.duration_ms, retries => r.retries, commit => r.commit.as_ref().map(|c| context! { repo => c.key.repo.as_str(), sha => c.key.sha.as_str(), short => c.key.sha.short(), origin => c.origin.as_str(), resolved => c.resolved }), message => r.message.clone(), run => r.run.as_str() })
        .collect();
    let artifacts: Vec<Value> = t
        .latest
        .iter()
        .flat_map(|r| r.artifacts.iter())
        .filter_map(|a| st.artifacts.get(a))
        .map(|a| context! { id => a.id.as_str(), kind => a.kind.as_str(), role => a.role.as_str(), name => a.name.clone(), url => artifact_url(&st, a), exists => a.exists })
        .collect();
    let suite = st.suites.iter().find(|s| s.id == t.test.suite);
    let run = t.latest.as_ref().and_then(|r| st_run(&st, &r.run));
    render(
        &app,
        &hx,
        "pages/test_detail.html",
        &t.test.name,
        "tests",
        context! {
            t => test_row(&st, t),
            test => Value::from_serialize(&t.test),
            latest => t.latest.as_ref().map(Value::from_serialize),
            history => history,
            artifacts => artifacts,
            suite => suite.map(Value::from_serialize),
            run => run,
            duration_trend => t.duration_trend.map(|(p, l, r)| context! { previous => p, latest => l, rel => r }),
            entity_id => t.test.id.as_str(),
        },
    )
}

fn st_run(_st: &ProjectState, _run: &crate::ids::RunId) -> Option<Value> {
    None
}

pub fn artifact_url(st: &ProjectState, a: &Artifact) -> String {
    match &a.path {
        ArtifactPath::Url(u) => u.clone(),
        ArtifactPath::Project(p) => {
            let repo = repo_of_path(st, p.as_str());
            let prefix = st
                .project
                .repos
                .iter()
                .find(|r| r.id.as_str() == repo)
                .map(|r| r.path.clone())
                .unwrap_or_else(crate::paths::RelPath::root);
            let inner = p.strip_prefix(&prefix).unwrap_or_else(|| p.clone());
            format!("/artifacts/{}/{}", super::view::urlencode(&repo), inner.as_str())
        }
        ArtifactPath::EvidenceCache(p) => format!("/artifacts/evidence/{}", p.as_str()),
        ArtifactPath::External(p) => format!("/artifacts/external/{}", p.display()),
    }
}

pub fn feature_detail_json(st: &ProjectState, v: &FeatureView) -> serde_json::Value {
    serde_json::json!({
        "feature": v.feature,
        "health": v.health,
        "impl_commit": v.impl_commit,
        "impl_committed_at": v.impl_committed_at.map(|t| t.to_string()),
        "commits": v.commits.iter().take(20).map(|c| serde_json::json!({ "repo": c.key.repo, "sha": c.key.sha, "origin": c.origin.key(), "note": c.origin.note(), "at": c.committed_at.to_string(), "subject": st.commits.get(&c.key).map(|x| x.commit.subject.clone()) })).collect::<Vec<_>>(),
        "adrs": v.adrs.iter().map(|a| serde_json::json!({ "id": a, "title": st.adrs.get(a).map(|x| x.adr.title.clone()), "status": st.adrs.get(a).map(|x| x.adr.status.as_str()) })).collect::<Vec<_>>(),
        "tests": v.tests, "benchmarks": v.benchmarks, "visuals": v.visuals,
        "links": { "html": format!("/features/{}", super::view::urlencode(v.feature.id.as_str())), "api": format!("/api/v1/features/{}", super::view::urlencode(v.feature.id.as_str())) },
    })
}

async fn feature_detail(
    hx: Hx,
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let Some(v) = st.feature(&id) else {
        let suggestions = suggest(&st, &id);
        return Err(AppError::NotFound { kind: "feature", id, suggestions });
    };
    let f = &v.feature;
    let commits: Vec<Value> = v
        .commits
        .iter()
        .take(10)
        .filter_map(|c| st.commits.get(&c.key).map(|cv| (c, cv)))
        .map(|(c, cv)| context! { repo => c.key.repo.as_str(), sha => c.key.sha.as_str(), short => c.key.sha.short(), subject => cv.commit.subject.clone(), at => c.committed_at.to_string(), explicit => c.origin.is_explicit(), note => c.origin.note() })
        .collect();
    let adrs: Vec<Value> = v
        .adrs
        .iter()
        .filter_map(|a| st.adrs.get(a))
        .map(|a| context! { id => a.adr.id.as_str(), title => a.adr.title.clone(), status => a.adr.status.as_str(), date => a.adr.date.map(|d| d.to_string()) })
        .collect();
    let adr_paths: HashMap<String, String> = st
        .adrs
        .values()
        .map(|a| (a.adr.source.as_str().to_string(), a.adr.id.as_str().to_string()))
        .collect();
    let feature_paths: HashMap<String, String> = st
        .features
        .values()
        .map(|x| (x.feature.source.as_str().to_string(), x.feature.id.as_str().to_string()))
        .collect();
    let repo = repo_of_path(&st, f.source.as_str());
    let base_dir = f.source.parent();
    let lc = super::markdown::LinkContext {
        base_dir: base_dir.as_str(),
        repo: &repo,
        adr_paths: &adr_paths,
        feature_paths: &feature_paths,
    };
    let body_html = super::markdown::render(&strip_first_h1(&f.body_md), Some(&lc));
    let rollups: Vec<Value> = EvidenceCategory::ALL
        .iter()
        .map(|cat| {
            let r = v.health.rollups.iter().find(|r| r.category == *cat);
            context! {
                label => cat.label(),
                status => r.map(|r| r.status.as_str()).unwrap_or("missing"),
                summary => r.map(|r| r.summary.clone()).unwrap_or_else(|| "no evidence".into()),
                required => r.map(|r| r.required).unwrap_or(false),
                freshness => r.and_then(|r| r.freshness.as_ref()).map(|fr| context! { change => format!("{:?}", fr.change).to_ascii_lowercase(), note => fr.note.clone(), against => fr.against.as_ref().map(|c| c.sha.short().to_string()) }),
            }
        })
        .collect();
    render(
        &app,
        &hx,
        "pages/feature_detail.html",
        &f.title,
        "features",
        context! {
            f => feature_row(&st, v),
            feature => Value::from_serialize(f),
            health => Value::from_serialize(&v.health),
            rollups => rollups,
            body_html => Value::from_safe_string(body_html),
            commits => commits,
            adrs => adrs,
            impl_commit => v.impl_commit.as_ref().map(|c| context! { repo => c.repo.as_str(), sha => c.sha.as_str(), short => c.sha.short(), at => v.impl_committed_at.map(|t| t.to_string()), subject => st.commits.get(c).map(|x| x.commit.subject.clone()) }),
            entity_id => f.id.as_str(),
            sources => Value::from_serialize(&st.sources),
            source_path => f.source.as_str(),
            tests => v.tests.iter().filter_map(|t| st.tests.get(t)).take(50).map(|t| test_row(&st, t)).collect::<Vec<_>>(),
            tests_total => v.tests.len(),
        },
    )
}

fn strip_first_h1(md: &str) -> String {
    let lines = md.lines();
    let mut out: Vec<&str> = Vec::new();
    let mut skipped = false;
    for l in lines {
        if !skipped && l.starts_with("# ") {
            skipped = true;
            continue;
        }
        out.push(l);
    }
    out.join("\n")
}

fn repo_of_path(st: &ProjectState, path: &str) -> String {
    let rel = crate::paths::RelPath::new(path);
    let mut best: Option<&Repo> = None;
    for r in &st.project.repos {
        if rel.starts_with(&r.path)
            && best.map(|b| r.path.as_str().len() > b.path.as_str().len()).unwrap_or(true)
        {
            best = Some(r);
        }
    }
    best.map(|r| r.id.as_str().to_string()).unwrap_or_else(|| "root".into())
}

fn suggest(st: &ProjectState, id: &str) -> Vec<(String, String)> {
    let q = id.to_ascii_lowercase();
    let mut v: Vec<(String, String)> = st
        .features
        .values()
        .filter(|f| {
            f.feature.id.as_str().to_ascii_lowercase().contains(&q)
                || f.feature.title.to_ascii_lowercase().contains(&q)
        })
        .map(|f| (f.feature.id.as_str().to_string(), f.feature.title.clone()))
        .collect();
    v.truncate(5);
    v
}

// ── Attention ───────────────────────────────────────────────────────────────

async fn attention(
    hx: Hx,
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let show_info = q.get("include").map(|s| s == "informational").unwrap_or(false);
    let rows: Vec<Value> = st
        .attention
        .iter()
        .filter(|a| show_info || a.severity != Severity::Informational)
        .map(|a| attention_row(&st, a))
        .collect();
    let info_count = st.attention.iter().filter(|a| a.severity == Severity::Informational).count();
    render(
        &app,
        &hx,
        "pages/attention.html",
        "Attention",
        "attention",
        context! { rows => rows, info_count => info_count, show_info => show_info },
    )
}

// ── Changes / commits ───────────────────────────────────────────────────────

async fn changes(
    hx: Hx,
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let limit: usize = q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(100);
    let rows = change_rows(
        &st,
        q.get("feature").map(|s| s.as_str()),
        q.get("repo").map(|s| s.as_str()),
        limit,
    );
    let repos: Vec<String> = st
        .project
        .repos
        .iter()
        .filter(|r| r.head.is_some())
        .map(|r| r.id.as_str().to_string())
        .collect();
    render(
        &app,
        &hx,
        "pages/changes.html",
        "Changes",
        "changes",
        context! { rows => rows, repos => repos, feature => q.get("feature"), repo => q.get("repo"), limit => limit, total => st.commits.len() },
    )
}

async fn commit(
    hx: Hx,
    State(app): State<AppState>,
    Path(sha): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let matches: Vec<_> = st
        .find_commits(&sha)
        .into_iter()
        .filter(|c| q.get("repo").map(|r| c.commit.key.repo.as_str() == r).unwrap_or(true))
        .collect();
    let c = match matches.as_slice() {
        [one] => *one,
        [] => return Err(AppError::NotFound { kind: "commit", id: sha, suggestions: vec![] }),
        many => {
            let choices: Vec<Value> = many.iter().map(|c| commit_row(&st, c)).collect();
            return render(
                &app,
                &hx,
                "pages/commit_choose.html",
                "Which commit?",
                "changes",
                context! { sha => sha, choices => choices },
            );
        }
    };
    let files: Vec<Value> = c
        .commit
        .files
        .iter()
        .map(|f| context! { path => f.path.as_str(), repo_path => f.repo_path.clone(), kind => f.kind.as_str(), added => f.added, deleted => f.deleted })
        .collect();
    let body_html = super::markdown::render(&c.commit.body, None);
    let adr_refs: Vec<Value> = c
        .adr_refs
        .iter()
        .map(|a| {
            let resolved = st.adrs.get(a).or_else(|| st.adrs.values().find(|x| x.adr.number.is_some() && x.adr.number == a.number()));
            context! { id => a.as_str(), resolved_id => resolved.map(|r| r.adr.id.as_str().to_string()), title => resolved.map(|r| r.adr.title.clone()) }
        })
        .collect();
    render(
        &app,
        &hx,
        "pages/commit.html",
        &format!("{} {}", c.commit.short(), c.commit.subject),
        "changes",
        context! { c => commit_row(&st, c), commit => Value::from_serialize(&c.commit), files => files, body_html => Value::from_safe_string(body_html), adr_refs => adr_refs, entity_id => c.commit.key.sha.as_str() },
    )
}

// ── ADRs ────────────────────────────────────────────────────────────────────

async fn adrs(
    hx: Hx,
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let status = q.get("status").filter(|s| !s.is_empty() && *s != "all").cloned();
    let mut rows: Vec<&crate::state::AdrView> = st
        .adrs
        .values()
        .filter(|a| status.as_ref().map(|s| a.adr.status.as_str() == s).unwrap_or(true))
        .collect();
    rows.sort_by(|a, b| b.adr.number.cmp(&a.adr.number).then_with(|| a.adr.id.cmp(&b.adr.id)));
    let rows: Vec<Value> = rows.into_iter().map(|a| adr_row(&st, a)).collect();
    render(
        &app,
        &hx,
        "pages/adrs.html",
        "ADRs",
        "adrs",
        context! { rows => rows, status => status, counts => Value::from_serialize(&st.counts) },
    )
}

fn adr_row(st: &ProjectState, a: &crate::state::AdrView) -> Value {
    let feats: Vec<Value> = a
        .features
        .iter()
        .map(|f| context! { id => f.as_str(), title => st.features.get(f).map(|x| x.feature.title.clone()).unwrap_or_else(|| f.to_string()), state => st.features.get(f).map(|x| x.health.state.as_str()).unwrap_or("unknown") })
        .collect();
    context! {
        id => a.adr.id.as_str(),
        number => a.adr.number,
        title => a.adr.title.clone(),
        status => a.adr.status.as_str(),
        status_raw => a.adr.status_raw.clone(),
        date => a.adr.date.map(|d| d.to_string()),
        accepted_at => a.adr.accepted_at.map(|d| d.to_string()),
        superseded_by => a.adr.superseded_by.as_ref().map(|s| s.as_str().to_string()),
        supersedes => a.adr.supersedes.iter().map(|s| s.as_str().to_string()).collect::<Vec<_>>(),
        features => feats,
        source => a.adr.source.as_str(),
        repo => repo_of_path(st, a.adr.source.as_str()),
        citing => a.citing_commits.len(),
        evidence_notes => a.adr.evidence_notes.iter().map(|e| context! { heading => e.heading.clone(), date => e.date.map(|d| d.to_string()) }).collect::<Vec<_>>(),
    }
}

async fn adr_detail(
    hx: Hx,
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let key = AdrId::parse_ref(&id).unwrap_or_else(|| AdrId::new(id.clone()));
    let Some(a) = st.adrs.get(&key).or_else(|| {
        st.adrs.values().find(|a| a.adr.number.is_some() && a.adr.number == key.number())
    }) else {
        return Err(AppError::NotFound { kind: "adr", id, suggestions: vec![] });
    };
    let adr_paths: HashMap<String, String> = st
        .adrs
        .values()
        .map(|x| (x.adr.source.as_str().to_string(), x.adr.id.as_str().to_string()))
        .collect();
    let feature_paths: HashMap<String, String> = st
        .features
        .values()
        .map(|x| (x.feature.source.as_str().to_string(), x.feature.id.as_str().to_string()))
        .collect();
    let repo = repo_of_path(&st, a.adr.source.as_str());
    let base_dir = a.adr.source.parent();
    let lc = super::markdown::LinkContext {
        base_dir: base_dir.as_str(),
        repo: &repo,
        adr_paths: &adr_paths,
        feature_paths: &feature_paths,
    };
    let body_html = super::markdown::render(&strip_first_h1(&a.adr.body_md), Some(&lc));
    let citing: Vec<Value> = a
        .citing_commits
        .iter()
        .filter_map(|k| st.commits.get(k))
        .take(10)
        .map(|c| commit_row(&st, c))
        .collect();
    let toc: Vec<Value> = a.adr.sections.iter().filter(|s| s.level == 2).map(|s| context! { heading => s.heading.clone(), anchor => format!("h-{}", slugify(&s.heading)) }).collect();
    render(
        &app,
        &hx,
        "pages/adr_detail.html",
        &format!("{} {}", a.adr.id, a.adr.title),
        "adrs",
        context! { a => adr_row(&st, a), body_html => Value::from_safe_string(body_html), citing => citing, toc => toc, entity_id => a.adr.id.as_str() },
    )
}

fn slugify(s: &str) -> String {
    // comrak's header id algorithm: lowercase, strip punctuation, spaces → hyphens.
    let mut out = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if c == ' ' || c == '-' || c == '_' {
            out.push('-');
        }
    }
    out
}

// ── Status / search ─────────────────────────────────────────────────────────

async fn status(hx: Hx, State(app): State<AppState>) -> Result<Page, AppError> {
    let st = app.state.load();
    render(
        &app,
        &hx,
        "pages/status.html",
        "Status",
        "status",
        context! {
            repos => Value::from_serialize(&st.project.repos),
            sources => Value::from_serialize(&st.sources),
            warnings => Value::from_serialize(&st.warnings),
            unresolved => Value::from_serialize(&st.unresolved),
            last_scan_duration_ms => st.last_scan_duration_ms,
            root => st.project.root.display().to_string(),
        },
    )
}

pub fn search_groups(st: &ProjectState, query: &str, limit: usize) -> Vec<serde_json::Value> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return vec![];
    }
    let mut groups = Vec::new();
    let feats: Vec<serde_json::Value> = st
        .features
        .values()
        .filter(|f| f.feature.id.as_str().to_ascii_lowercase().contains(&q) || f.feature.title.to_ascii_lowercase().contains(&q))
        .take(limit)
        .map(|f| serde_json::json!({ "id": f.feature.id, "title": f.feature.title, "subtitle": f.health.state.label(), "url": format!("/features/{}", super::view::urlencode(f.feature.id.as_str())), "state": f.health.state.as_str() }))
        .collect();
    if !feats.is_empty() {
        groups.push(serde_json::json!({ "kind": "Features", "hits": feats }));
    }
    let adrs: Vec<serde_json::Value> = st
        .adrs
        .values()
        .filter(|a| a.adr.id.as_str().to_ascii_lowercase().contains(&q) || a.adr.title.to_ascii_lowercase().contains(&q))
        .take(limit)
        .map(|a| serde_json::json!({ "id": a.adr.id, "title": a.adr.title, "subtitle": a.adr.status.as_str(), "url": format!("/adrs/{}", super::view::urlencode(a.adr.id.as_str())) }))
        .collect();
    if !adrs.is_empty() {
        groups.push(serde_json::json!({ "kind": "ADRs", "hits": adrs }));
    }
    let commits: Vec<serde_json::Value> = st
        .recent_commits
        .iter()
        .filter_map(|k| st.commits.get(k))
        .filter(|c| c.commit.key.sha.as_str().starts_with(&q) || c.commit.subject.to_ascii_lowercase().contains(&q))
        .take(limit)
        .map(|c| serde_json::json!({ "id": c.commit.short(), "title": c.commit.subject, "subtitle": c.commit.key.repo, "url": format!("/commits/{}?repo={}", c.commit.key.sha, super::view::urlencode(c.commit.key.repo.as_str())) }))
        .collect();
    if !commits.is_empty() {
        groups.push(serde_json::json!({ "kind": "Commits", "hits": commits }));
    }
    groups
}

async fn search(
    hx: Hx,
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page, AppError> {
    let st = app.state.load();
    let query = q.get("q").cloned().unwrap_or_default();
    let groups = search_groups(&st, &query, 20);
    render(
        &app,
        &hx,
        "pages/search.html",
        "Search",
        "",
        context! { q => query, groups => Value::from_serialize(&groups) },
    )
}

#[allow(dead_code)]
fn _unused(_: &FeatureId) {}
