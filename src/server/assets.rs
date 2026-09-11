//! Static assets embedded in the binary (disk in `--dev`), served with long-lived caching
//! when requested with their content hash.

use super::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;
use std::collections::HashMap;

#[derive(RustEmbed)]
#[folder = "ui/assets/"]
struct UiAssets;

#[derive(RustEmbed)]
#[folder = "ui/vendor/"]
struct VendorAssets;

pub async fn serve(
    State(app): State<AppState>,
    Path(path): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let (bytes, from_disk): (Vec<u8>, bool) = if let Some(dir) = &app.ui_dir {
        let p = if let Some(rest) = path.strip_prefix("vendor/") {
            dir.join("vendor").join(rest)
        } else {
            dir.join("assets").join(&path)
        };
        match std::fs::read(&p) {
            Ok(b) => (b, true),
            Err(_) => return StatusCode::NOT_FOUND.into_response(),
        }
    } else {
        let file = if let Some(rest) = path.strip_prefix("vendor/") {
            VendorAssets::get(rest)
        } else {
            UiAssets::get(&path)
        };
        match file {
            Some(f) => (f.data.to_vec(), false),
            None => return StatusCode::NOT_FOUND.into_response(),
        }
    };
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    let mut res = bytes.into_response();
    let h = res.headers_mut();
    let ct = if mime.type_() == "text" || mime.subtype() == "javascript" || mime.subtype() == "json"
    {
        format!("{mime}; charset=utf-8")
    } else {
        mime.to_string()
    };
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&ct).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    let cache = if from_disk {
        "no-store"
    } else if q.contains_key("v") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    res
}
