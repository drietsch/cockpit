//! File discovery. Documents are walked gitignore-aware; evidence globs get their own walk
//! later (M1) because they live under ignored build directories.

use crate::config::Located;
use crate::paths::RelPath;
use std::path::{Path, PathBuf};

fn walk_md(root: &Path, dir: &Path, excludes: &[String]) -> Vec<PathBuf> {
    if !dir.is_dir() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut builder = ignore::WalkBuilder::new(dir);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .follow_links(false)
        .sort_by_file_name(|a, b| a.cmp(b));
    for entry in builder.build().flatten() {
        let p = entry.path();
        if !p.is_file() || p.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('_') || excludes.iter().any(|x| x == name) {
            continue;
        }
        if let Some(rel) = crate::paths::relativize(root, p) {
            if rel.components().any(|c| c == "node_modules" || c == ".git") {
                continue;
            }
        }
        out.push(p.to_path_buf());
    }
    out
}

pub fn feature_files(located: &Located) -> Vec<PathBuf> {
    let dir = RelPath::new(&located.config.features.path).to_abs(&located.root);
    walk_md(&located.root, &dir, &["README.md".to_string(), "index.md".to_string()])
}

pub fn adr_files(located: &Located) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in located.config.adrs.path.items() {
        let dir = RelPath::new(&p).to_abs(&located.root);
        out.extend(walk_md(&located.root, &dir, &located.config.adrs.exclude));
    }
    out
}

/// Every `Cargo.toml` and `package.json` in the project (gitignore-aware), for the workspace
/// indexes.
pub fn manifests(located: &Located) -> Vec<RelPath> {
    let mut out = Vec::new();
    let mut builder = ignore::WalkBuilder::new(&located.root);
    builder.hidden(true).git_ignore(true).git_global(false).follow_links(false).max_depth(Some(8));
    builder.filter_entry(|e| {
        let n = e.file_name().to_string_lossy();
        n != "node_modules" && n != "target" && n != ".git" && n != "vendor"
    });
    for entry in builder.build().flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "Cargo.toml" || name == "package.json" {
            if let Some(rel) = crate::paths::relativize(&located.root, p) {
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}
