mod support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use support::ProjectBuilder;
use tower::ServiceExt;

fn project() -> ProjectBuilder {
    ProjectBuilder::new()
        .feature(
            "FEAT-001",
            "Search",
            &["src/search/**"],
            "importance: critical\nadrs:\n  - ADR-0001\n",
            "Search things.",
        )
        .adr(
            1,
            "Use an inverted index",
            "Accepted",
            "2026-02-01",
            "Because [FEAT-001] needs it. See [the spec](../spec/x.md).",
        )
        .file("src/search/index.rs", "fn a() {}\n")
        .commit("Replace search index [FEAT-001]", "2026-03-02T09:00:00Z")
}

async fn get(app: &axum::Router, path: &str, hx: bool) -> (StatusCode, String, Option<String>) {
    let mut req = Request::builder().uri(path);
    if hx {
        req = req.header("HX-Request", "true");
    }
    let res = app.clone().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status();
    let push = res.headers().get("HX-Push-Url").map(|v| v.to_str().unwrap().to_string());
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).to_string(), push)
}

#[tokio::test]
async fn every_page_renders_full_and_partial() {
    let p = project();
    let c = p.scan();
    let app = cockpit::server::build_router(cockpit::server::AppState::standalone(&c).unwrap());
    let head = p.head_sha();
    let pages = [
        ("/", "Needs attention"),
        ("/features", "Search"),
        ("/features/FEAT-001", "Evidence freshness"),
        ("/attention", "Attention"),
        ("/changes", "Replace search index"),
        (&format!("/commits/{head}"), "Affected features"),
        ("/adrs", "Use an inverted index"),
        ("/adrs/ADR-0001", "Affected features"),
        ("/status", "Import warnings"),
        ("/search?q=search", "Search"),
        ("/tests", "Not in this build"),
    ];
    for (path, needle) in pages {
        let (status, body, _) = get(&app, path, false).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(body.contains("<!doctype html>"), "{path} should be a full page");
        assert!(body.contains("<aside"), "{path} should include the sidebar");
        assert!(body.contains(needle), "{path} should contain {needle:?}");
        assert!(!body.contains("<script>"), "{path} must not carry inline scripts (CSP)");
        let (status, body, _) = get(&app, path, true).await;
        assert_eq!(status, StatusCode::OK, "{path} partial");
        assert!(!body.contains("<!doctype html>"), "{path} partial must not be a full page");
        assert!(!body.contains("<aside"), "{path} partial must not include chrome");
        assert!(body.contains("<title>"), "{path} partial carries a title for htmx");
        assert!(body.contains("hx-swap-oob"), "{path} partial updates the chrome out of band");
    }
}

#[tokio::test]
async fn fragments_push_canonical_urls_and_errors_are_specific() {
    let p = project();
    let c = p.scan();
    let app = cockpit::server::build_router(cockpit::server::AppState::standalone(&c).unwrap());
    let (status, body, push) =
        get(&app, "/fragments/features/table?state=unknown&sort=name", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(push.as_deref(), Some("/features?state=unknown&sort=name"));
    assert!(!body.contains("<html"));
    assert!(body.contains("id=\"features-table\""));
    let (status, body, _) = get(&app, "/features/NOPE", false).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("No feature with id NOPE"));
    let (status, body, _) = get(&app, "/api/v1/features/FEAT-001", false).await;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["data"]["health"]["state"], "unknown");
    assert!(json["generation"].as_u64().unwrap() >= 1);
    let (status, body, _) = get(&app, "/adrs/ADR-0001", false).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("/files/"), "relative ADR links are rewritten to the file viewer");
    let (status, _, _) = get(&app, "/assets/css/tokens.css", false).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = get(&app, "/assets/vendor/htmx.min.js", false).await;
    assert_eq!(status, StatusCode::OK);
}
