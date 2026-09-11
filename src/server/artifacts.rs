//! `/artifacts/{root}/{*path}`: read-only serving of evidence files (screenshots, videos,
//! traces, reports) from a closed set of roots — each repository, the evidence cache and the
//! explicitly configured external roots. Never the umbrella project root, never secrets.

use super::AppState;
use axum::body::Body;
use axum::extract::{Path as AxPath, State};
use axum::http::{HeaderValue, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::path::{Component, Path, PathBuf};
use tower::ServiceExt;

/// Served roots, resolved once at startup. `external` is special: the path after it is
/// absolute and must lie under one of the allowed external directories.
#[derive(Clone, Debug, Default)]
pub struct ArtifactRoots {
    pub named: Vec<(String, PathBuf)>,
    pub external: Vec<PathBuf>,
}

impl ArtifactRoots {
    pub fn from_located(
        located: &crate::config::Located,
        repos: &[crate::model::Repo],
    ) -> ArtifactRoots {
        let mut named: Vec<(String, PathBuf)> = Vec::new();
        for r in repos {
            named.push((r.id.as_str().to_string(), located.root.join(r.path.as_str())));
        }
        // A single-repo project at the root may also be addressed as `root`.
        if located.root.join(".git").exists() && !named.iter().any(|(n, _)| n == "root") {
            named.push(("root".into(), located.root.clone()));
        }
        named.push(("evidence".into(), located.evidence_dir()));
        let mut external: Vec<PathBuf> = located
            .config
            .server
            .artifact_roots
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .collect();
        for src in located.evidence_sources() {
            if !src.allow_outside_root {
                continue;
            }
            for pat in src.paths() {
                if pat.starts_with('/') {
                    let lit: String = pat.split(['*', '?', '[']).next().unwrap_or("").to_string();
                    if let Some((d, _)) = lit.rsplit_once('/') {
                        external.push(PathBuf::from(d));
                    }
                }
            }
        }
        ArtifactRoots { named, external }
    }
}

const ALLOWED_EXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "svg", "avif", "bmp", "mp4", "webm", "mov", "zip", "json",
    "xml", "txt", "log", "md", "csv", "html", "htm", "pdf", "har", "trace",
];

const SECRET_NAMES: &[&str] =
    &["credentials", "id_rsa", "id_ed25519", "id_ecdsa", ".npmrc", ".netrc", ".htpasswd"];

fn looks_secret(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with(".env")
        || n.ends_with(".pem")
        || n.ends_with(".key")
        || n.ends_with(".p12")
        || n.ends_with(".pfx")
        || n.ends_with(".token")
        || n.contains("token")
        || n.contains("secret")
        || n.contains("password")
        || SECRET_NAMES.iter().any(|s| n.starts_with(s))
}

fn reject(status: StatusCode, msg: &'static str) -> Response {
    (status, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], msg).into_response()
}

/// Validate and resolve `/artifacts/{root}/{path}` to a file on disk.
pub fn resolve(
    roots: &ArtifactRoots,
    root: &str,
    path: &str,
) -> Result<PathBuf, (StatusCode, &'static str)> {
    let raw = path.replace('\\', "/");
    if raw.is_empty() || raw.contains('\0') {
        return Err((StatusCode::BAD_REQUEST, "bad path"));
    }
    let rel = Path::new(&raw);
    for c in rel.components() {
        match c {
            Component::Normal(seg) => {
                let s = seg.to_string_lossy();
                if s == ".git" || s == ".cockpit" && root != "evidence" {
                    return Err((StatusCode::FORBIDDEN, "forbidden"));
                }
                if looks_secret(&s) {
                    return Err((StatusCode::FORBIDDEN, "forbidden"));
                }
            }
            Component::ParentDir
            | Component::CurDir
            | Component::RootDir
            | Component::Prefix(_) => {
                if !(root == "external" && matches!(c, Component::RootDir)) {
                    return Err((StatusCode::FORBIDDEN, "forbidden"));
                }
            }
        }
    }
    let ext = rel
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !ALLOWED_EXT.contains(&ext.as_str()) {
        return Err((StatusCode::UNSUPPORTED_MEDIA_TYPE, "file type is not served"));
    }
    let (base, candidate) = if root == "external" {
        let abs = PathBuf::from(format!("/{}", raw.trim_start_matches('/')));
        let base = roots
            .external
            .iter()
            .find(|b| abs.starts_with(b))
            .cloned()
            .ok_or((StatusCode::NOT_FOUND, "unknown root"))?;
        (base, abs)
    } else {
        let base = roots
            .named
            .iter()
            .find(|(n, _)| n == root)
            .map(|(_, p)| p.clone())
            .ok_or((StatusCode::NOT_FOUND, "unknown root"))?;
        (base.clone(), base.join(rel))
    };
    let canon_base = base.canonicalize().map_err(|_| (StatusCode::NOT_FOUND, "unknown root"))?;
    let canon = candidate.canonicalize().map_err(|_| (StatusCode::NOT_FOUND, "not found"))?;
    if !canon.starts_with(&canon_base) {
        return Err((StatusCode::FORBIDDEN, "forbidden"));
    }
    if !canon.is_file() {
        return Err((StatusCode::NOT_FOUND, "not found"));
    }
    Ok(canon)
}

pub async fn serve(
    State(app): State<AppState>,
    AxPath((root, path)): AxPath<(String, String)>,
    req: Request<Body>,
) -> Response {
    let file = match resolve(&app.artifact_roots, &root, &path) {
        Ok(f) => f,
        Err((status, msg)) => return reject(status, msg),
    };
    let mut res = match tower_http::services::ServeFile::new(&file).oneshot(req).await {
        Ok(r) => r.into_response(),
        Err(_) => return reject(StatusCode::INTERNAL_SERVER_ERROR, "could not read file"),
    };
    let h = res.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=60"));
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(ext.as_str(), "html" | "htm" | "svg") {
        h.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "sandbox; default-src 'none'; img-src data:; style-src 'unsafe-inline'",
            ),
        );
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(dir: &Path) -> ArtifactRoots {
        ArtifactRoots {
            named: vec![
                ("core".into(), dir.join("core")),
                ("evidence".into(), dir.join(".cockpit/evidence")),
            ],
            external: vec![dir.join("ext")],
        }
    }

    #[test]
    fn resolves_inside_roots_and_refuses_escapes_and_secrets() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir_all(d.join("core/test-results")).unwrap();
        std::fs::write(d.join("core/test-results/shot.png"), b"png").unwrap();
        std::fs::write(d.join("core/.env"), b"SECRET=1").unwrap();
        std::fs::write(d.join("core/main.rs"), b"fn main(){}").unwrap();
        std::fs::create_dir_all(d.join("ext/run")).unwrap();
        std::fs::write(d.join("ext/run/heat.png"), b"png").unwrap();
        std::fs::write(d.join("outside.png"), b"png").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(d.join("outside.png"), d.join("core/link.png")).unwrap();
        let r = roots(d);
        assert!(resolve(&r, "core", "test-results/shot.png").is_ok());
        assert_eq!(resolve(&r, "core", "../outside.png").unwrap_err().0, StatusCode::FORBIDDEN);
        assert_eq!(resolve(&r, "core", ".env").unwrap_err().0, StatusCode::FORBIDDEN);
        assert_eq!(
            resolve(&r, "core", "main.rs").unwrap_err().0,
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        assert_eq!(resolve(&r, "nope", "x.png").unwrap_err().0, StatusCode::NOT_FOUND);
        assert_eq!(resolve(&r, "core", "missing.png").unwrap_err().0, StatusCode::NOT_FOUND);
        #[cfg(unix)]
        assert_eq!(
            resolve(&r, "core", "link.png").unwrap_err().0,
            StatusCode::FORBIDDEN,
            "symlink escapes are refused"
        );
        let ext = d.join("ext/run/heat.png").to_string_lossy().to_string();
        assert!(resolve(&r, "external", &ext).is_ok());
        let out = d.join("outside.png").to_string_lossy().to_string();
        assert_eq!(resolve(&r, "external", &out).unwrap_err().0, StatusCode::NOT_FOUND);
    }
}
