//! Web server: Axum routes → application services over the `ProjectState` snapshot →
//! MiniJinja templates. HTMX gives SPA-like navigation; SSE pushes live updates.

pub mod api;
pub mod artifacts;
pub mod assets;
pub mod hx;
pub mod markdown;
pub mod perf;
pub mod routes;
pub mod sse;
pub mod templates;
pub mod view;
pub mod visual;

use crate::cockpit::Cockpit;
use crate::state::StateHandle;
use axum::Router;
use axum::http::{HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;

pub struct ServeOptions {
    pub bind: String,
    pub port: u16,
    pub open: bool,
    pub dev: bool,
    pub watch: bool,
}

/// Events pushed to browsers over SSE.
#[derive(Clone, Debug, serde::Serialize)]
pub struct LiveEvent {
    pub seq: u64,
    pub name: String,
    pub payload: serde_json::Value,
}

#[derive(Clone)]
pub struct AppState {
    pub state: StateHandle,
    pub templates: Arc<templates::Templates>,
    pub dev: bool,
    pub ui_dir: Option<PathBuf>,
    pub events: broadcast::Sender<LiveEvent>,
    pub seq: Arc<std::sync::atomic::AtomicU64>,
    pub project_root: PathBuf,
    pub evidence_dir: PathBuf,
    pub scan_requests: tokio::sync::mpsc::Sender<()>,
    pub multi_repo: bool,
    pub artifact_roots: artifacts::ArtifactRoots,
}

impl AppState {
    pub fn emit(&self, name: &str, payload: serde_json::Value) {
        let seq = self.seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let _ = self.events.send(LiveEvent { seq, name: name.to_string(), payload });
    }
}

pub fn build_router(app: AppState) -> Router {
    Router::new()
        .merge(routes::router())
        .merge(perf::router())
        .merge(visual::router())
        .nest("/api/v1", api::router())
        .route("/events", axum::routing::get(sse::events))
        .route("/assets/{*path}", axum::routing::get(assets::serve))
        .route("/artifacts/{root}/{*path}", axum::routing::get(artifacts::serve))
        .route("/healthz", axum::routing::get(|| async { "ok" }))
        .fallback(routes::not_found)
        .layer(middleware::from_fn(security_headers))
        .layer(tower_http::compression::CompressionLayer::new())
        .with_state(app)
}

async fn security_headers(req: axum::extract::Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'",
        ),
    );
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    res
}

/// Run the server on the current thread (builds a Tokio runtime). Rescans run on a
/// dedicated thread owned by this function; the `Cockpit` is moved in.
pub fn serve_blocking(c: &mut Cockpit, opts: ServeOptions) -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let (events, _) = broadcast::channel::<LiveEvent>(256);
    let (scan_tx, mut scan_rx) = tokio::sync::mpsc::channel::<()>(4);
    let ui_dir =
        if opts.dev { Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui")) } else { None };
    let templates = Arc::new(templates::Templates::new(ui_dir.clone())?);
    templates.check_all()?;
    let app = AppState {
        state: c.state.clone(),
        templates,
        dev: opts.dev,
        ui_dir,
        events: events.clone(),
        seq: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        project_root: c.located.root.clone(),
        evidence_dir: c.located.evidence_dir(),
        scan_requests: scan_tx,
        multi_repo: c.located.config.repos.len() > 1 || !c.located.root.join(".git").exists(),
        artifact_roots: artifacts::ArtifactRoots::from_located(
            &c.located,
            &c.store.repos().unwrap_or_default(),
        ),
    };

    // Rescan worker: owns the Cockpit for the lifetime of the server.
    let located = c.located.clone();
    let index_path = c.store.path.clone();
    let state_handle = c.state.clone();
    let app_for_scan = app.clone();
    let watch = opts.watch;
    let scan_thread = std::thread::spawn(move || {
        let mut cockpit = match Cockpit::from_located(located, Some(&index_path)) {
            Ok(k) => k,
            Err(e) => {
                tracing::error!("scan worker could not open the project: {e}");
                return;
            }
        };
        // Share the same handle so the server sees the worker's states.
        cockpit.state = state_handle;
        let mut watcher =
            if watch { crate::scan::watch::Watcher::start(&cockpit.located).ok() } else { None };
        loop {
            let wake = rt.block_on(async {
                tokio::select! {
                    r = scan_rx.recv() => r.map(|_| "request"),
                    _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => Some("tick"),
                }
            });
            let Some(kind) = wake else { break };
            let due = match kind {
                "request" => true,
                _ => watcher.as_mut().map(|w| w.take_changes()).unwrap_or(false),
            };
            if !due {
                continue;
            }
            app_for_scan.emit("project.scan.started", serde_json::json!({}));
            let before = cockpit.state.load();
            match cockpit.scan(None) {
                Ok(report) => {
                    let after = cockpit.state.load();
                    app_for_scan.emit(
                        "project.scan.completed",
                        serde_json::json!({
                            "generation": after.generation,
                            "duration_ms": report.duration_ms,
                            "warnings": report.warnings.len(),
                            "headline": after.headline,
                            "overall": after.overall.as_str(),
                        }),
                    );
                    for (id, v) in &after.features {
                        let prev = before.features.get(id).map(|p| p.health.state);
                        if prev != Some(v.health.state) {
                            app_for_scan.emit(
                                "feature.health.changed",
                                serde_json::json!({ "feature_id": id, "title": v.feature.title, "from": prev.map(|p| p.as_str()), "to": v.health.state.as_str(), "headline": v.health.headline }),
                            );
                        }
                    }
                }
                Err(e) => tracing::error!("rescan failed: {e}"),
            }
        }
    });

    let rt2 = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let router = build_router(app);
    let addr: SocketAddr = format!("{}:{}", opts.bind, opts.port).parse()?;
    rt2.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        if opts.open {
            let url = format!("http://{}:{}/", opts.bind, opts.port);
            let _ = std::process::Command::new(if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            })
            .arg(&url)
            .spawn();
        }
        axum::serve(listener, router).with_graceful_shutdown(shutdown()).await?;
        Ok::<(), anyhow::Error>(())
    })?;
    drop(scan_thread);
    Ok(())
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

impl AppState {
    /// An app state without a scan worker, for tests and embedding.
    pub fn standalone(cockpit: &Cockpit) -> anyhow::Result<AppState> {
        let (events, _) = broadcast::channel::<LiveEvent>(16);
        let (scan_tx, _rx) = tokio::sync::mpsc::channel::<()>(1);
        let templates = Arc::new(templates::Templates::new(None)?);
        Ok(AppState {
            state: cockpit.state.clone(),
            templates,
            dev: false,
            ui_dir: None,
            events,
            seq: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            project_root: cockpit.located.root.clone(),
            evidence_dir: cockpit.located.evidence_dir(),
            scan_requests: scan_tx,
            multi_repo: cockpit.located.config.repos.len() > 1
                || !cockpit.located.root.join(".git").exists(),
            artifact_roots: artifacts::ArtifactRoots::from_located(
                &cockpit.located,
                &cockpit.store.repos().unwrap_or_default(),
            ),
        })
    }
}
