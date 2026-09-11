//! JSON API: the same services as the HTML pages, one envelope.

use super::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use std::collections::HashMap;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/project", get(project))
        .route("/status", get(status))
        .route("/features", get(features))
        .route("/features/{id}", get(feature))
        .route("/attention", get(attention))
        .route("/adrs", get(adrs))
        .route("/adrs/{id}", get(adr))
        .route("/benchmarks", get(benchmarks))
        .route("/benchmarks/{id}", get(benchmark))
        .route("/benchmarks/{id}/history", get(benchmark_history))
        .route("/changes", get(changes))
        .route("/commits/{sha}", get(commit))
        .route("/search", get(search))
        .route("/query", get(query))
        .route("/scan", post(scan))
}

fn to_json(v: minijinja::Value) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
}

fn envelope(app: &AppState, data: serde_json::Value) -> Response {
    let st = app.state.load();
    Json(serde_json::json!({ "data": data, "generation": st.generation, "generated_at": st.generated_at.to_string() })).into_response()
}

fn not_found(kind: &str, id: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": { "code": "not_found", "message": format!("no {kind} {id}") } }))).into_response()
}

async fn project(State(app): State<AppState>) -> Response {
    let st = app.state.load();
    envelope(
        &app,
        serde_json::json!({ "project": st.project, "overall": st.overall.as_str(), "headline": st.headline, "generation": st.generation, "scanned_at": st.last_scan_at.map(|t| t.to_string()) }),
    )
}

async fn status(State(app): State<AppState>) -> Response {
    let st = app.state.load();
    envelope(
        &app,
        serde_json::json!({ "overall": st.overall.as_str(), "headline": st.headline, "counts": st.counts, "repos": st.project.repos, "warnings": st.warnings, "sources": st.sources, "last_scan_at": st.last_scan_at.map(|t| t.to_string()), "version": crate::VERSION }),
    )
}

async fn features(
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let st = app.state.load();
    // `?query=<expr>` runs the query language; the other parameters are the page filters.
    if let Some(expr) = q.get("query").filter(|s| !s.trim().is_empty()) {
        let parsed = match crate::query::parse(&format!("features where {expr}")) {
            Ok(p) => p,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": { "code": "bad_query", "message": e } })),
                )
                    .into_response();
            }
        };
        let rows = match crate::query::run(&st, &parsed) {
            Ok(crate::query::Rows::Features(v)) => v
                .into_iter()
                .map(|v| to_json(super::routes::feature_row(&st, v)))
                .collect::<Vec<_>>(),
            Ok(_) => vec![],
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": { "code": "bad_query", "message": e } })),
                )
                    .into_response();
            }
        };
        let n = rows.len();
        return envelope(&app, serde_json::json!({ "items": rows, "total": n, "query": expr }));
    }
    let filter = super::routes::FeatureFilter::from_query(&q);
    let rows: Vec<serde_json::Value> = super::routes::filtered_features(&st, &filter)
        .into_iter()
        .map(|v| to_json(super::routes::feature_row(&st, v)))
        .collect();
    envelope(&app, serde_json::json!({ "items": rows, "total": st.features.len() }))
}

async fn feature(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    let st = app.state.load();
    match st.feature(&id) {
        Some(v) => envelope(&app, super::routes::feature_detail_json(&st, v)),
        None => not_found("feature", &id),
    }
}

async fn benchmarks(
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let st = app.state.load();
    let filter = match super::perf::PerfFilter::from_query(&q) {
        Ok(f) => f,
        Err(e) => return bad_request(&e),
    };
    let rows: Vec<serde_json::Value> = super::perf::filtered_benchmarks(&st, &filter)
        .into_iter()
        .map(|b| to_json(super::perf::benchmark_row(&st, b)))
        .collect();
    envelope(&app, serde_json::json!({ "items": rows, "total": st.benchmarks.len() }))
}

async fn benchmark(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let st = app.state.load();
    match st.benchmarks.get(&crate::ids::BenchmarkId::new(id.clone())) {
        Some(b) => envelope(
            &app,
            super::perf::benchmark_detail_json(
                &st,
                b,
                q.get("range").map(|s| s.as_str()).unwrap_or("all"),
            ),
        ),
        None => not_found("benchmark", &id),
    }
}

async fn benchmark_history(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    let st = app.state.load();
    match st.benchmarks.get(&crate::ids::BenchmarkId::new(id.clone())) {
        Some(b) => {
            envelope(&app, serde_json::json!({ "items": b.history, "total": b.history.len() }))
        }
        None => not_found("benchmark", &id),
    }
}

fn bad_request(e: &super::hx::AppError) -> Response {
    let msg = match e {
        super::hx::AppError::BadQuery(m) => m.clone(),
        other => format!("{other:?}"),
    };
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": { "code": "bad_request", "message": msg } })),
    )
        .into_response()
}

/// `GET /api/v1/query?q=<full query>` — any entity, ids and rows.
async fn query(State(app): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let st = app.state.load();
    let expr = q.get("q").cloned().unwrap_or_default();
    let parsed = match crate::query::parse(&expr) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": { "code": "bad_query", "message": e } })),
            )
                .into_response();
        }
    };
    let (entity, items): (String, Vec<serde_json::Value>) = match crate::query::run(&st, &parsed) {
        Ok(crate::query::Rows::Features(v)) => ("features".into(), v.into_iter().map(|v| to_json(super::routes::feature_row(&st, v))).collect()),
        Ok(crate::query::Rows::Tests(v)) => ("tests".into(), v.into_iter().map(|t| to_json(super::routes::test_row(&st, t))).collect()),
        Ok(crate::query::Rows::Benchmarks(v)) => ("benchmarks".into(), v.into_iter().map(|b| to_json(super::perf::benchmark_row(&st, b))).collect()),
        Ok(crate::query::Rows::Adrs(v)) => ("adrs".into(), v.into_iter().map(|a| serde_json::json!({ "id": a.adr.id, "title": a.adr.title, "status": a.adr.status.as_str(), "features": a.features })).collect()),
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": { "code": "bad_query", "message": e } }))).into_response(),
    };
    let n = items.len();
    envelope(
        &app,
        serde_json::json!({ "query": expr, "entity": entity, "count": n, "items": items }),
    )
}

async fn attention(State(app): State<AppState>) -> Response {
    let st = app.state.load();
    envelope(&app, serde_json::json!({ "items": st.attention, "total": st.attention.len() }))
}

async fn adrs(State(app): State<AppState>) -> Response {
    let st = app.state.load();
    let items: Vec<serde_json::Value> = st
        .adrs
        .values()
        .map(|a| serde_json::json!({ "id": a.adr.id, "number": a.adr.number, "title": a.adr.title, "status": a.adr.status.as_str(), "date": a.adr.date.map(|d| d.to_string()), "superseded_by": a.adr.superseded_by, "features": a.features, "source": a.adr.source }))
        .collect();
    envelope(&app, serde_json::json!({ "items": items, "total": st.adrs.len() }))
}

async fn adr(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let st = app.state.load();
    let key =
        crate::ids::AdrId::parse_ref(&id).unwrap_or_else(|| crate::ids::AdrId::new(id.clone()));
    let Some(a) = st.adrs.get(&key).or_else(|| {
        st.adrs.values().find(|a| a.adr.number.is_some() && a.adr.number == key.number())
    }) else {
        return not_found("adr", &id);
    };
    let mut v = serde_json::to_value(a).unwrap_or_default();
    if q.get("include").map(|s| s.as_str()) != Some("body") {
        if let Some(o) = v.get_mut("adr").and_then(|x| x.as_object_mut()) {
            o.remove("body_md");
        }
    }
    envelope(&app, v)
}

async fn changes(
    State(app): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let st = app.state.load();
    let limit: usize = q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(50);
    let items: Vec<serde_json::Value> = super::routes::change_rows(
        &st,
        q.get("feature").map(|s| s.as_str()),
        q.get("repo").map(|s| s.as_str()),
        limit,
    )
    .into_iter()
    .map(to_json)
    .collect();
    envelope(&app, serde_json::json!({ "items": items }))
}

async fn commit(
    State(app): State<AppState>,
    Path(sha): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let st = app.state.load();
    let matches: Vec<_> = st
        .find_commits(&sha)
        .into_iter()
        .filter(|c| q.get("repo").map(|r| c.commit.key.repo.as_str() == r).unwrap_or(true))
        .collect();
    match matches.as_slice() {
        [one] => envelope(&app, serde_json::to_value(one).unwrap_or_default()),
        [] => not_found("commit", &sha),
        many => (StatusCode::MULTIPLE_CHOICES, Json(serde_json::json!({ "error": { "code": "ambiguous", "message": "sha prefix matches several commits", "matches": many.iter().map(|c| serde_json::json!({"repo": c.commit.key.repo, "sha": c.commit.key.sha})).collect::<Vec<_>>() } }))).into_response(),
    }
}

async fn search(State(app): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let st = app.state.load();
    let query = q.get("q").cloned().unwrap_or_default();
    envelope(&app, serde_json::json!({ "groups": super::routes::search_groups(&st, &query, 8) }))
}

async fn scan(State(app): State<AppState>, headers: axum::http::HeaderMap) -> Response {
    if headers.get("x-cockpit").is_none() {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({ "error": { "code": "forbidden", "message": "missing X-Cockpit header" } }))).into_response();
    }
    let _ = app.scan_requests.try_send(());
    (StatusCode::ACCEPTED, Json(serde_json::json!({ "accepted": true }))).into_response()
}
