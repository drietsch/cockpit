//! `/files/{repo}/{*path}`: read-only source viewer with line anchors. `root` addresses the
//! project root; paths are project-relative there and repo-relative otherwise. Secrets and
//! anything outside the tree are refused like artifacts.

use super::AppState;
use super::hx::{AppError, Hx, Page, render};
use axum::extract::{Path, State};
use minijinja::{Value, context};
use std::path::{Component, PathBuf};

const MAX_BYTES: u64 = 2 * 1024 * 1024;

fn resolve(app: &AppState, repo: &str, path: &str) -> Result<(PathBuf, String), AppError> {
    let raw = path.replace('\\', "/");
    if raw.is_empty() || raw.contains('\0') {
        return Err(AppError::BadQuery("bad path".into()));
    }
    for c in std::path::Path::new(&raw).components() {
        match c {
            Component::Normal(seg) => {
                let s = seg.to_string_lossy();
                if s == ".git"
                    || s == ".cockpit"
                    || s.starts_with(".env")
                    || s.ends_with(".pem")
                    || s.ends_with(".key")
                    || s.contains("token")
                    || s.contains("secret")
                {
                    return Err(AppError::NotFound {
                        kind: "file",
                        id: raw.clone(),
                        suggestions: vec![],
                    });
                }
            }
            _ => {
                return Err(AppError::NotFound {
                    kind: "file",
                    id: raw.clone(),
                    suggestions: vec![],
                });
            }
        }
    }
    let base = if repo == "root" {
        app.project_root.clone()
    } else {
        app.artifact_roots
            .named
            .iter()
            .find(|(n, _)| n == repo)
            .map(|(_, p)| p.clone())
            .ok_or_else(|| AppError::NotFound {
                kind: "repository",
                id: repo.to_string(),
                suggestions: vec![],
            })?
    };
    let canon_base = base.canonicalize().map_err(|_| AppError::NotFound {
        kind: "repository",
        id: repo.to_string(),
        suggestions: vec![],
    })?;
    let canon = base.join(&raw).canonicalize().map_err(|_| AppError::NotFound {
        kind: "file",
        id: raw.clone(),
        suggestions: vec![],
    })?;
    if !canon.starts_with(&canon_base) || !canon.is_file() {
        return Err(AppError::NotFound { kind: "file", id: raw.clone(), suggestions: vec![] });
    }
    // On an umbrella project the root itself is not a repository: only files inside one of the
    // repositories are viewable (the umbrella may hold local secrets).
    if repo == "root" {
        let inside_repo = app.artifact_roots.named.iter().any(|(n, p)| {
            n != "evidence" && p.canonicalize().map(|b| canon.starts_with(b)).unwrap_or(false)
        });
        if !inside_repo {
            return Err(AppError::NotFound { kind: "file", id: raw.clone(), suggestions: vec![] });
        }
    }
    Ok((canon, raw))
}

pub async fn file(
    hx: Hx,
    State(app): State<AppState>,
    Path((repo, path)): Path<(String, String)>,
) -> Result<Page, AppError> {
    let (abs, rel) = resolve(&app, &repo, &path)?;
    let meta = std::fs::metadata(&abs).map_err(|e| AppError::Internal(e.to_string()))?;
    let ext = abs
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let is_image = matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif");
    let (lines, truncated, binary): (Vec<String>, bool, bool) = if is_image
        || meta.len() > MAX_BYTES
    {
        (vec![], meta.len() > MAX_BYTES, false)
    } else {
        let bytes = std::fs::read(&abs).map_err(|e| AppError::Internal(e.to_string()))?;
        if bytes.iter().take(8000).any(|b| *b == 0) {
            (vec![], false, true)
        } else {
            (String::from_utf8_lossy(&bytes).lines().map(|l| l.to_string()).collect(), false, false)
        }
    };
    let image_url = if is_image {
        Some(format!(
            "/artifacts/{}/{}",
            super::view::urlencode(if repo == "root" {
                app.artifact_roots
                    .named
                    .iter()
                    .find(|(_, p)| *p == app.project_root)
                    .map(|(n, _)| n.as_str())
                    .unwrap_or("root")
            } else {
                &repo
            }),
            rel
        ))
    } else {
        None
    };
    let lang = match ext.as_str() {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "mjs" => "javascript",
        "md" => "markdown",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "html" => "html",
        "css" => "css",
        "sh" => "shell",
        "py" => "python",
        _ => "text",
    };
    let crumbs: Vec<Value> = {
        let mut acc = String::new();
        rel.split('/')
            .map(|seg| {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(seg);
                context! { name => seg, path => acc.clone() }
            })
            .collect()
    };
    render(
        &app,
        &hx,
        "pages/file.html",
        &rel,
        "changes",
        context! {
            repo => repo,
            path => rel,
            crumbs => crumbs,
            lines => lines,
            line_count => lines.len(),
            truncated => truncated,
            binary => binary,
            size => meta.len(),
            lang => lang,
            image_url => image_url,
            entity_id => rel,
        },
    )
}
