//! HTMX-aware rendering: a boosted navigation gets the `<main>` (plus out-of-band chrome
//! bits) instead of the full page; fragments get only themselves.

use super::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};

#[derive(Clone, Debug, Default)]
pub struct Hx {
    pub request: bool,
    pub boosted: bool,
    pub history_restore: bool,
    pub target: Option<String>,
    pub current_url: Option<String>,
    pub theme: String,
}

impl Hx {
    pub fn partial(&self) -> bool {
        self.request && !self.history_restore
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Hx {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let h = &parts.headers;
        let get = |k: &str| h.get(k).and_then(|v| v.to_str().ok()).map(|s| s.to_string());
        let theme = h
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|c| {
                c.split(';')
                    .map(|p| p.trim())
                    .find_map(|p| p.strip_prefix("cockpit_theme="))
                    .map(|t| t.to_string())
            })
            .filter(|t| t == "light" || t == "dark")
            .unwrap_or_else(|| "dark".into());
        Ok(Hx {
            theme,
            request: get("HX-Request").as_deref() == Some("true"),
            boosted: get("HX-Boosted").as_deref() == Some("true"),
            history_restore: get("HX-History-Restore-Request").as_deref() == Some("true"),
            target: get("HX-Target"),
            current_url: get("HX-Current-URL"),
        })
    }
}

/// A rendered page or fragment.
pub struct Page {
    pub html: String,
    pub push_url: Option<String>,
    pub status: StatusCode,
}

impl IntoResponse for Page {
    fn into_response(self) -> Response {
        let mut res = (self.status, Html(self.html)).into_response();
        let h = res.headers_mut();
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        h.insert(header::VARY, HeaderValue::from_static("HX-Request"));
        if let Some(u) = self.push_url {
            if let Ok(v) = HeaderValue::from_str(&u) {
                h.insert("HX-Push-Url", v);
            }
        }
        res
    }
}

fn shell_ctx(app: &AppState) -> minijinja::Value {
    let st = app.state.load();
    minijinja::context! {
        project => minijinja::Value::from_serialize(&st.project),
        headline => st.headline.clone(),
        overall => st.overall.as_str(),
        counts => minijinja::Value::from_serialize(&st.counts),
        attention_count => st.attention.iter().filter(|a| a.severity != crate::model::Severity::Informational).count(),
        warnings_count => st.warnings.len(),
        generation => st.generation,
        last_scan_at => st.last_scan_at.map(|t| t.to_string()),
        multi_repo => app.multi_repo,
    }
}

/// Render a page template with the standard context; `partial` decides whether chrome is
/// included.
pub fn render(
    app: &AppState,
    hx: &Hx,
    template: &str,
    title: &str,
    nav: &str,
    ctx: minijinja::Value,
) -> Result<Page, AppError> {
    let full = minijinja::context! {
        partial => hx.partial(),
        theme => hx.theme.clone(),
        page_title => title,
        nav => nav,
        shell => shell_ctx(app),
        dev => app.dev,
        version => crate::VERSION,
        ..ctx
    };
    let html = app.templates.render(template, full)?;
    Ok(Page { html, push_url: None, status: StatusCode::OK })
}

pub fn fragment(
    app: &AppState,
    template: &str,
    ctx: minijinja::Value,
    push_url: Option<String>,
) -> Result<Page, AppError> {
    let html = app.templates.render(
        template,
        minijinja::context! { partial => true, shell => shell_ctx(app), ..ctx },
    )?;
    Ok(Page { html, push_url, status: StatusCode::OK })
}

#[derive(Debug)]
pub enum AppError {
    NotFound { kind: &'static str, id: String, suggestions: Vec<(String, String)> },
    BadQuery(String),
    Template(minijinja::Error),
    Internal(String),
}

impl From<minijinja::Error> for AppError {
    fn from(e: minijinja::Error) -> Self {
        AppError::Template(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, title, message) = match &self {
            AppError::NotFound { kind, id, .. } => {
                (StatusCode::NOT_FOUND, "Not found".to_string(), format!("No {kind} with id {id}"))
            }
            AppError::BadQuery(m) => {
                (StatusCode::BAD_REQUEST, "Bad request".to_string(), m.clone())
            }
            AppError::Template(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Template error".to_string(), e.to_string())
            }
            AppError::Internal(m) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Error".to_string(), m.clone())
            }
        };
        tracing::debug!("{status}: {message}");
        let body = format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title><link rel=\"stylesheet\" href=\"/assets/css/tokens.css\"><link rel=\"stylesheet\" href=\"/assets/css/app.css\"></head><body class=\"error-page\"><main class=\"error\"><h1>{title}</h1><p>{}</p><p><a href=\"/\">Overview</a> · <a href=\"/status\">Status</a></p></main></body></html>",
            html_escape(&message)
        );
        (status, Html(body)).into_response()
    }
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn wants_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|a| a.starts_with("application/json"))
        .unwrap_or(false)
}
