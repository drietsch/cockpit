//! Read-only discovery of a repository's build layout for `cockpit plan`: Moon projects
//! (directory, languages, `dependsOn`) and the Cargo package and workspace that own a file.
//!
//! Everything here reads files; nothing starts a process. Moon's own project graph is not
//! queried: `moon query` writes Moon's cache into the worktree and can take minutes, while
//! the edges a plan needs are declared in each project's `moon.yml`.

use anyhow::{Context, Result};
use serde_yaml_ng::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

/// One Moon project as a plan sees it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MoonProject {
    pub dir: String,
    /// The project inherits the `rust-*` tasks.
    pub rust: bool,
    /// The project inherits the `js-*` tasks.
    pub js: bool,
    /// Where the languages came from: `tags`, `language`, `manifests`, or `unknown` (then a
    /// source file's extension decides, as before projects were read).
    pub languages_from: &'static str,
    pub depends_on: BTreeSet<String>,
}

pub type MoonProjects = BTreeMap<String, MoonProject>;

const RUST_TAGS: &[&str] = &["rust"];
const JS_TAGS: &[&str] = &["node", "bun", "deno", "javascript", "typescript", "js", "ts"];

/// The projects named by `.moon/workspace.yml` (`projects:` as an id → directory map), each
/// with what its `moon.yml` says. `None` when the repository has no Moon workspace.
pub fn read_moon(root: &Path) -> Result<Option<MoonProjects>> {
    let path = root.join(".moon/workspace.yml");
    if !path.is_file() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let workspace: Value =
        serde_yaml_ng::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let mut projects = MoonProjects::new();
    if let Some(map) = workspace.get("projects").and_then(Value::as_mapping) {
        for (id, dir) in map {
            let (Some(id), Some(dir)) = (id.as_str(), dir.as_str()) else { continue };
            let dir = dir.trim_start_matches("./").trim_end_matches('/').to_owned();
            projects.insert(id.to_owned(), read_project(root, &dir)?);
        }
    }
    Ok(Some(projects))
}

fn read_project(root: &Path, dir: &str) -> Result<MoonProject> {
    let base = root.join(dir);
    let mut project = MoonProject { dir: dir.to_owned(), ..Default::default() };
    let config = ["moon.yml", "moon.yaml"].iter().map(|n| base.join(n)).find(|p| p.is_file());
    let doc = match config {
        Some(path) => {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            serde_yaml_ng::from_str::<Value>(&text)
                .with_context(|| format!("parsing {}", path.display()))?
        }
        None => Value::Null,
    };
    let tags: Vec<String> = doc
        .get("tags")
        .and_then(Value::as_sequence)
        .map(|s| s.iter().filter_map(Value::as_str).map(str::to_ascii_lowercase).collect())
        .unwrap_or_default();
    let tagged = |set: &[&str]| tags.iter().any(|t| set.contains(&t.as_str()));
    let language = doc.get("language").and_then(Value::as_str).map(str::to_ascii_lowercase);
    if tagged(RUST_TAGS) || tagged(JS_TAGS) {
        project.rust = tagged(RUST_TAGS);
        project.js = tagged(JS_TAGS);
        project.languages_from = "tags";
    } else if let Some(language) = language.as_deref().filter(|l| *l != "unknown") {
        project.rust = language == "rust";
        project.js = matches!(language, "typescript" | "javascript");
        project.languages_from = "language";
    } else {
        project.rust = base.join("Cargo.toml").is_file();
        project.js = base.join("package.json").is_file();
        project.languages_from = if project.rust || project.js { "manifests" } else { "unknown" };
    }
    if let Some(deps) = doc.get("dependsOn").and_then(Value::as_sequence) {
        for dep in deps {
            let id = dep.as_str().or_else(|| dep.get("id").and_then(Value::as_str));
            if let Some(id) = id {
                project.depends_on.insert(id.to_owned());
            }
        }
    }
    Ok(project)
}

/// Projects that depend on any of `changed`, following `dependsOn` backwards: one step for
/// `deep == false`, all of them otherwise. A changed project is included when another changed
/// project reaches it: its own change may cover one language, the dependency all of them.
pub fn dependents(
    projects: &MoonProjects,
    changed: &BTreeSet<String>,
    deep: bool,
) -> BTreeSet<String> {
    let mut reverse = BTreeMap::<&str, Vec<&str>>::new();
    for (id, project) in projects {
        for dep in &project.depends_on {
            reverse.entry(dep.as_str()).or_default().push(id.as_str());
        }
    }
    let mut found = BTreeSet::new();
    let mut queue: VecDeque<(&str, usize)> = changed.iter().map(|id| (id.as_str(), 0)).collect();
    while let Some((id, depth)) = queue.pop_front() {
        if !deep && depth == 1 {
            continue;
        }
        for &dependent in reverse.get(id).into_iter().flatten() {
            if found.insert(dependent.to_owned()) {
                queue.push_back((dependent, depth + 1));
            }
        }
    }
    found
}

/// The Cargo workspace (repository-relative directory) a file builds in, and the package that
/// owns it when there is one (`None` for a workspace-level `Cargo.toml` or `Cargo.lock`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CargoOwner {
    pub workspace_dir: String,
    pub package: Option<String>,
}

#[derive(Default)]
struct Manifest {
    package: Option<String>,
    workspace: bool,
    exclude: Vec<String>,
}

fn read_manifest(path: &Path) -> Option<Manifest> {
    let text = std::fs::read_to_string(path).ok()?;
    let doc: toml::Table = toml::from_str(&text).ok()?;
    let package =
        doc.get("package").and_then(|p| p.get("name")).and_then(|n| n.as_str()).map(str::to_owned);
    let workspace = doc.get("workspace");
    let exclude = workspace
        .and_then(|w| w.get("exclude"))
        .and_then(|e| e.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).map(|s| s.trim_end_matches('/').to_owned()))
        .map(Iterator::collect)
        .unwrap_or_default();
    Some(Manifest { package, workspace: workspace.is_some(), exclude })
}

fn join(dir: &[&str]) -> String {
    if dir.is_empty() { ".".to_owned() } else { dir.join("/") }
}

/// Walk up from a changed file to the nearest `Cargo.toml` with a `[package]` and the
/// nearest one with a `[workspace]` that does not exclude that package. Reads manifests
/// only; no `cargo metadata`.
pub fn cargo_owner(root: &Path, file: &str) -> Option<CargoOwner> {
    let parts: Vec<&str> = file.split('/').collect();
    let name = *parts.last()?;
    // A manifest or lockfile belongs to its own directory; any other file to its parents.
    let start = parts.len() - 1;
    let mut package: Option<(usize, String)> = None;
    let manifest_itself = name == "Cargo.toml" || name == "Cargo.lock";
    for depth in (0..=start).rev() {
        let dir = &parts[..depth];
        let Some(manifest) = read_manifest(&root.join(join(dir)).join("Cargo.toml")) else {
            continue;
        };
        if package.is_none() {
            if let Some(name) = &manifest.package {
                package = Some((depth, name.clone()));
            }
        }
        if manifest.workspace {
            if let Some((pkg_depth, _)) = &package {
                let relative = parts[depth..*pkg_depth].join("/");
                if manifest
                    .exclude
                    .iter()
                    .any(|e| relative == *e || relative.starts_with(&format!("{e}/")))
                {
                    // An excluded package builds on its own.
                    let (pkg_depth, pkg) = package.unwrap();
                    return Some(CargoOwner {
                        workspace_dir: join(&parts[..pkg_depth]),
                        package: Some(pkg),
                    });
                }
            }
            let package = package.map(|(_, name)| name);
            // The workspace root's own manifest or lockfile is a workspace-wide input.
            let package = if manifest_itself && depth == start { None } else { package };
            return Some(CargoOwner { workspace_dir: join(dir), package });
        }
    }
    package.map(|(depth, name)| CargoOwner {
        workspace_dir: join(&parts[..depth]),
        package: Some(name),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &Path, path: &str, text: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn layout() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        write(
            r,
            ".moon/workspace.yml",
            "projects:\n  core: engine/core\n  image: \"modules/image\"\n  editor: apps/editor\n  tests: tests\n  plain: tools/plain\n",
        );
        write(r, "engine/core/moon.yml", "language: rust\ntags: [\"rust\", \"wasm\"]\n");
        write(r, "engine/core/package.json", "{}");
        write(
            r,
            "modules/image/moon.yml",
            "tags: [rust, node]\ndependsOn:\n  - id: core\n    scope: production\n",
        );
        write(r, "apps/editor/moon.yml", "language: typescript\ndependsOn: [core, image]\n");
        write(r, "tests/moon.yml", "tags: [node]\ndependsOn:\n  - editor\n");
        write(r, "tools/plain/Cargo.toml", "[package]\nname = \"plain\"\n");
        write(
            r,
            "engine/core/Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"spikes/solo\"]\n",
        );
        write(r, "engine/core/crates/paged-geom/Cargo.toml", "[package]\nname = \"paged-geom\"\n");
        write(r, "engine/core/spikes/solo/Cargo.toml", "[package]\nname = \"solo\"\n");
        write(r, "modules/image/Cargo.toml", "[package]\nname = \"image-root\"\n[workspace]\n");
        t
    }

    #[test]
    fn languages_come_from_tags_then_language_then_manifests() {
        let t = layout();
        let p = read_moon(t.path()).unwrap().unwrap();
        // core has a package.json, but its tags say Rust only.
        assert_eq!((p["core"].rust, p["core"].js, p["core"].languages_from), (true, false, "tags"));
        assert_eq!((p["image"].rust, p["image"].js), (true, true));
        assert_eq!(
            (p["editor"].rust, p["editor"].js, p["editor"].languages_from),
            (false, true, "language")
        );
        assert_eq!(
            (p["plain"].rust, p["plain"].js, p["plain"].languages_from),
            (true, false, "manifests")
        );
    }

    #[test]
    fn dependents_follow_depends_on_backwards() {
        let t = layout();
        let p = read_moon(t.path()).unwrap().unwrap();
        let core = BTreeSet::from(["core".to_owned()]);
        assert_eq!(dependents(&p, &core, false), BTreeSet::from(["image".into(), "editor".into()]));
        assert_eq!(
            dependents(&p, &core, true),
            BTreeSet::from(["image".into(), "editor".into(), "tests".into()])
        );
        assert!(dependents(&p, &BTreeSet::from(["tests".to_owned()]), true).is_empty());
        // editor changed too, but core's change still reaches it.
        let both = BTreeSet::from(["core".to_owned(), "editor".to_owned()]);
        assert!(dependents(&p, &both, false).contains("editor"));
    }

    #[test]
    fn cargo_owner_finds_package_and_workspace() {
        let t = layout();
        let r = t.path();
        let owner = |f: &str| cargo_owner(r, f);
        assert_eq!(
            owner("engine/core/crates/paged-geom/src/lib.rs"),
            Some(CargoOwner {
                workspace_dir: "engine/core".into(),
                package: Some("paged-geom".into())
            })
        );
        assert_eq!(
            owner("engine/core/crates/paged-geom/Cargo.toml").unwrap().package.as_deref(),
            Some("paged-geom")
        );
        assert_eq!(
            owner("engine/core/Cargo.lock"),
            Some(CargoOwner { workspace_dir: "engine/core".into(), package: None })
        );
        assert_eq!(
            owner("engine/core/Cargo.toml"),
            Some(CargoOwner { workspace_dir: "engine/core".into(), package: None })
        );
        assert_eq!(
            owner("engine/core/spikes/solo/src/main.rs"),
            Some(CargoOwner {
                workspace_dir: "engine/core/spikes/solo".into(),
                package: Some("solo".into())
            })
        );
        assert_eq!(
            owner("modules/image/src/lib.rs"),
            Some(CargoOwner {
                workspace_dir: "modules/image".into(),
                package: Some("image-root".into())
            })
        );
        assert_eq!(owner("tools/plain/src/lib.rs").unwrap().workspace_dir, "tools/plain");
        assert_eq!(owner("README.rs"), None);
    }
}
