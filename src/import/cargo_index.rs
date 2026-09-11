//! Cargo workspace index: package name → directory, and nextest `package::binary` classnames →
//! the test's source path. Built from `Cargo.toml` files alone (no `cargo metadata`), so it
//! works on any machine and inside tests.

use crate::model::TestKind;
use crate::paths::RelPath;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Default, Clone)]
pub struct CargoWorkspaceIndex {
    /// package name → package dir (project-root-relative)
    packages: HashMap<String, RelPath>,
    /// (package, target name) → declared path for `[[bin]]`/`[[test]]` overrides
    targets: HashMap<(String, String), RelPath>,
}

impl CargoWorkspaceIndex {
    pub fn build(root: &Path, manifests: &[RelPath]) -> CargoWorkspaceIndex {
        let mut idx = CargoWorkspaceIndex::default();
        for m in manifests {
            let abs = m.to_abs(root);
            let Ok(text) = std::fs::read_to_string(&abs) else { continue };
            let Ok(doc) = toml::from_str::<toml::Table>(&text) else { continue };
            let dir = m.parent();
            if let Some(name) =
                doc.get("package").and_then(|p| p.get("name")).and_then(|n| n.as_str())
            {
                idx.packages.insert(name.to_string(), dir.clone());
                for kind in ["bin", "test", "bench", "example"] {
                    if let Some(arr) = doc.get(kind).and_then(|v| v.as_array()) {
                        for t in arr {
                            if let (Some(n), Some(p)) = (
                                t.get("name").and_then(|v| v.as_str()),
                                t.get("path").and_then(|v| v.as_str()),
                            ) {
                                idx.targets.insert((name.to_string(), n.to_string()), dir.join(p));
                            }
                        }
                    }
                }
            }
        }
        idx
    }

    pub fn len(&self) -> usize {
        self.packages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }

    pub fn package_dir(&self, package: &str) -> Option<&RelPath> {
        self.packages.get(package)
    }

    /// Resolve a nextest test to its source: for lib/bin unit tests the module path in the
    /// test name (`search::index::tests::finds_things`) names the file; for integration
    /// binaries the binary does.
    pub fn resolve_test(
        &self,
        classname: &str,
        test_name: &str,
        root: Option<&Path>,
    ) -> Option<(RelPath, TestKind)> {
        let (path, kind) = self.resolve(classname, root)?;
        if kind != TestKind::Unit || !test_name.contains("::") {
            return Some((path, kind));
        }
        let src_dir = if path.as_str().ends_with("/src") || path.as_str() == "src" {
            path.clone()
        } else {
            path.parent()
        };
        let segs: Vec<&str> = test_name.split("::").collect();
        let mut modules: Vec<&str> = segs[..segs.len() - 1].to_vec();
        while modules.last().map(|m| *m == "tests" || *m == "test").unwrap_or(false) {
            modules.pop();
        }
        if modules.is_empty() {
            return Some((path, kind));
        }
        let exists = |p: &RelPath| root.map(|r| p.to_abs(r).exists()).unwrap_or(false);
        // Longest module prefix that exists as a file or directory.
        for n in (1..=modules.len()).rev() {
            let joined = modules[..n].join("/");
            let file = src_dir.join(&format!("{joined}.rs"));
            let dir_mod = src_dir.join(&format!("{joined}/mod.rs"));
            let dir = src_dir.join(&joined);
            if exists(&file) {
                return Some((file, kind));
            }
            if exists(&dir_mod) {
                return Some((dir_mod, kind));
            }
            if exists(&dir) {
                return Some((dir, kind));
            }
        }
        if root.is_none() {
            return Some((src_dir.join(&format!("{}.rs", modules.join("/"))), kind));
        }
        Some((path, kind))
    }

    /// Resolve a nextest binary id (`package`, `package::binary`, `package::bin/name`) to a
    /// source path and test kind. Paths are checked against the filesystem when `root` is
    /// given so `tests/<bin>.rs` vs `tests/<bin>/main.rs` is decided correctly.
    pub fn resolve(&self, classname: &str, root: Option<&Path>) -> Option<(RelPath, TestKind)> {
        let (package, binary) = match classname.split_once("::") {
            Some((p, b)) => (p, Some(b)),
            None => (classname, None),
        };
        let dir = self.packages.get(package)?;
        let exists = |p: &RelPath| root.map(|r| p.to_abs(r).exists()).unwrap_or(false);
        match binary {
            None => Some((dir.join("src"), TestKind::Unit)),
            Some(b) if b == package || b == "lib" => Some((dir.join("src"), TestKind::Unit)),
            Some("doctest") | Some("doctests") => Some((dir.join("src"), TestKind::Unit)),
            Some(b) if b.starts_with("bin/") => {
                let name = &b[4..];
                if let Some(p) = self.targets.get(&(package.to_string(), name.to_string())) {
                    return Some((p.clone(), TestKind::Unit));
                }
                let cand = dir.join(&format!("src/bin/{name}.rs"));
                Some((if exists(&cand) { cand } else { dir.join("src/main.rs") }, TestKind::Unit))
            }
            Some(b) => {
                if let Some(p) = self.targets.get(&(package.to_string(), b.to_string())) {
                    return Some((p.clone(), TestKind::Integration));
                }
                let flat = dir.join(&format!("tests/{b}.rs"));
                let nested = dir.join(&format!("tests/{b}/main.rs"));
                let p = if exists(&flat) {
                    flat
                } else if exists(&nested) {
                    nested
                } else if root.is_none() {
                    flat
                } else {
                    dir.join("tests")
                };
                Some((p, TestKind::Integration))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_nextest_classnames() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("crates/platform/guardian-retention/tests")).unwrap();
        std::fs::write(
            root.join("crates/platform/guardian-retention/Cargo.toml"),
            "[package]\nname = \"guardian-retention\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("crates/platform/guardian-retention/tests/retention_scale.rs"),
            "",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("crates/cli/guardian-cli/src/bin")).unwrap();
        std::fs::write(
            root.join("crates/cli/guardian-cli/Cargo.toml"),
            "[package]\nname = \"guardian-cli\"\n[[bin]]\nname = \"gdn\"\npath = \"src/gdn.rs\"\n",
        )
        .unwrap();
        let idx = CargoWorkspaceIndex::build(
            root,
            &[
                RelPath::new("crates/platform/guardian-retention/Cargo.toml"),
                RelPath::new("crates/cli/guardian-cli/Cargo.toml"),
            ],
        );
        assert_eq!(idx.len(), 2);
        let (p, k) = idx.resolve("guardian-retention::retention_scale", Some(root)).unwrap();
        assert_eq!(p.as_str(), "crates/platform/guardian-retention/tests/retention_scale.rs");
        assert_eq!(k, TestKind::Integration);
        let (p, k) = idx.resolve("guardian-retention", Some(root)).unwrap();
        assert_eq!(p.as_str(), "crates/platform/guardian-retention/src");
        assert_eq!(k, TestKind::Unit);
        let (p, _) = idx.resolve("guardian-cli::bin/gdn", Some(root)).unwrap();
        assert_eq!(p.as_str(), "crates/cli/guardian-cli/src/gdn.rs");
        std::fs::create_dir_all(root.join("crates/platform/guardian-retention/src/policy"))
            .unwrap();
        std::fs::write(root.join("crates/platform/guardian-retention/src/policy/mod.rs"), "")
            .unwrap();
        std::fs::write(root.join("crates/platform/guardian-retention/src/policy/eval.rs"), "")
            .unwrap();
        let (p, k) = idx
            .resolve_test("guardian-retention", "policy::eval::tests::walks_chain", Some(root))
            .unwrap();
        assert_eq!(p.as_str(), "crates/platform/guardian-retention/src/policy/eval.rs");
        assert_eq!(k, TestKind::Unit);
        let (p, _) =
            idx.resolve_test("guardian-retention", "policy::tests::x", Some(root)).unwrap();
        assert_eq!(p.as_str(), "crates/platform/guardian-retention/src/policy/mod.rs");
        let (p, _) =
            idx.resolve_test("guardian-retention::retention_scale", "walks", Some(root)).unwrap();
        assert_eq!(p.as_str(), "crates/platform/guardian-retention/tests/retention_scale.rs");
        assert!(idx.resolve("nope::x", Some(root)).is_none());
    }
}
