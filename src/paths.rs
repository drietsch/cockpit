//! Project-root-relative paths. Every path stored in the model is relative to the project
//! root and uses `/` separators, so that a multi-repo umbrella (`core/crates/x/src/y.rs`) and
//! a single repo (`src/y.rs`) are handled by the same glob machinery.

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RelPath(String);

impl RelPath {
    /// Build from a `/`-separated relative string; normalises `./` and empty segments.
    pub fn new(s: impl AsRef<str>) -> Self {
        let mut parts: Vec<&str> = Vec::new();
        for seg in s.as_ref().split(['/', '\\']) {
            match seg {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                other => parts.push(other),
            }
        }
        RelPath(parts.join("/"))
    }

    pub fn root() -> Self {
        RelPath(String::new())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    pub fn join(&self, tail: &str) -> RelPath {
        if self.0.is_empty() {
            RelPath::new(tail)
        } else {
            RelPath::new(format!("{}/{}", self.0, tail))
        }
    }

    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or("")
    }

    pub fn file_stem(&self) -> &str {
        let name = self.file_name();
        match name.rfind('.') {
            Some(0) | None => name,
            Some(i) => &name[..i],
        }
    }

    pub fn extension(&self) -> Option<&str> {
        let name = self.file_name();
        name.rfind('.').filter(|&i| i > 0).map(|i| &name[i + 1..])
    }

    pub fn parent(&self) -> RelPath {
        match self.0.rfind('/') {
            Some(i) => RelPath(self.0[..i].to_string()),
            None => RelPath::root(),
        }
    }

    /// True when `self` is `prefix` or lies beneath it.
    pub fn starts_with(&self, prefix: &RelPath) -> bool {
        if prefix.0.is_empty() {
            return true;
        }
        self.0 == prefix.0 || self.0.starts_with(&format!("{}/", prefix.0))
    }

    /// The part of `self` below `prefix`, if `self` starts with it.
    pub fn strip_prefix(&self, prefix: &RelPath) -> Option<RelPath> {
        if prefix.0.is_empty() {
            return Some(self.clone());
        }
        if self.0 == prefix.0 {
            return Some(RelPath::root());
        }
        self.0.strip_prefix(&format!("{}/", prefix.0)).map(|s| RelPath(s.to_string()))
    }

    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|s| !s.is_empty())
    }

    pub fn to_abs(&self, root: &Path) -> PathBuf {
        if self.0.is_empty() { root.to_path_buf() } else { root.join(&self.0) }
    }
}

impl std::fmt::Display for RelPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() { "." } else { &self.0 })
    }
}

impl std::fmt::Debug for RelPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RelPath({})", self)
    }
}

/// Relativise an absolute (or already relative) path against the project root. Returns `None`
/// when the path is not beneath the root.
pub fn relativize(root: &Path, path: &Path) -> Option<RelPath> {
    let rel = if path.is_absolute() { path.strip_prefix(root).ok()? } else { path };
    let mut s = String::new();
    for c in rel.components() {
        match c {
            Component::Normal(seg) => {
                if !s.is_empty() {
                    s.push('/');
                }
                s.push_str(&seg.to_string_lossy());
            }
            Component::CurDir => {}
            Component::ParentDir => return None,
            _ => return None,
        }
    }
    Some(RelPath(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_and_navigates() {
        let p = RelPath::new("./core/crates//x/src/lib.rs");
        assert_eq!(p.as_str(), "core/crates/x/src/lib.rs");
        assert_eq!(p.file_name(), "lib.rs");
        assert_eq!(p.file_stem(), "lib");
        assert_eq!(p.extension(), Some("rs"));
        assert_eq!(p.parent().as_str(), "core/crates/x/src");
        assert!(p.starts_with(&RelPath::new("core")));
        assert!(!p.starts_with(&RelPath::new("cor")));
        assert_eq!(p.strip_prefix(&RelPath::new("core/crates")).unwrap().as_str(), "x/src/lib.rs");
        assert_eq!(RelPath::new("docs/adr/0001-x.md").file_stem(), "0001-x");
    }

    #[test]
    fn relativizes() {
        let root = Path::new("/p");
        assert_eq!(relativize(root, Path::new("/p/a/b.md")).unwrap().as_str(), "a/b.md");
        assert!(relativize(root, Path::new("/q/a")).is_none());
        assert_eq!(relativize(root, Path::new("x/y")).unwrap().as_str(), "x/y");
    }
}
