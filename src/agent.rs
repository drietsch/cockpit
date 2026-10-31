//! Conservative, read-only change-impact and verification planning.
//!
//! A plan is not test evidence. It neither executes commands nor asserts that
//! selected checks cover the entire dependency graph. Existing CI is the gate.

use crate::config::{DevelopmentSection, RustCiBackend, RustEditBackend};
use crate::link::PathOwnership;
use crate::model::Feature;
use crate::paths::RelPath;
use crate::state::ProjectState;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use std::process::Command;

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
pub enum Mode {
    Edit,
    Checkpoint,
    Ci,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
pub enum FastBackend {
    /// Apply the shared development policy from cockpit.toml.
    Auto,
    None,
    Cargo,
    Blaze,
}

#[derive(Debug, Serialize)]
pub struct Action {
    pub stage: &'static str,
    pub cwd: String,
    pub command: Vec<String>,
    pub purpose: &'static str,
    pub authority: &'static str,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub mode: &'static str,
    pub source: String,
    pub changed_count: usize,
    pub changed_files: Vec<String>,
    pub omitted_changed_files: usize,
    pub projects: Vec<String>,
    pub unmapped_files: Vec<String>,
    pub unknown_inputs: Vec<String>,
    pub scope: &'static str,
    pub actions: Vec<Action>,
    pub authoritative_ci_required: bool,
    pub verification_status: &'static str,
    pub limitations: Vec<&'static str>,
    /// Optional bounded hints from a previous Cockpit scan, never verification of this edit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prior_evidence: Option<PriorEvidence>,
}

/// Path-linked feature and test hints from a *previous* scan.
///
/// The test results describe past runs only. This cannot justify skipping
/// verification of the current changed source snapshot.
#[derive(Debug, Serialize)]
pub struct PriorEvidence {
    pub provenance: &'static str,
    pub last_scan_at: Option<String>,
    pub state: &'static str,
    pub matched_feature_count: usize,
    pub omitted_features: usize,
    pub features: Vec<FeatureHint>,
    pub linked_test_count: usize,
    pub omitted_tests: usize,
    pub linked_tests: Vec<LinkedTestHint>,
    pub note: &'static str,
}

#[derive(Debug, Serialize)]
pub struct FeatureHint {
    pub id: String,
    pub title: String,
    pub importance: &'static str,
    pub matched_paths: Vec<String>,
    pub linked_test_count: usize,
    pub prior_health: &'static str,
}

#[derive(Debug, Serialize)]
pub struct LinkedTestHint {
    pub id: String,
    pub kind: &'static str,
    pub source_path: Option<String>,
    pub prior_status: Option<&'static str>,
}

fn feature_path_matches(features: &[Feature], changed: &[String]) -> BTreeMap<String, Vec<String>> {
    let ownership = PathOwnership::build(features);
    let mut matched = BTreeMap::<String, BTreeSet<String>>::new();
    for p in changed {
        for (id, _glob) in ownership.owners_of(&RelPath::new(p)) {
            matched.entry(id.as_str().to_owned()).or_default().insert(p.clone());
        }
    }
    matched.into_iter().map(|(id, paths)| (id, paths.into_iter().collect())).collect()
}

/// Enrich an already computed plan without a scan, a build, or a test.
/// Returned test status and feature health are historical: the worktree may
/// differ from the scan and even the same revision may have changed inputs.
pub fn attach_prior_evidence(plan: &mut Plan, state: &ProjectState, changed: &[String]) {
    const NOTE: &str = "Historical, potentially stale evidence only; no test was executed and no current change is verified.";
    let last_scan_at = state.last_scan_at.map(|t| t.to_string());
    if last_scan_at.is_none() {
        plan.prior_evidence = Some(PriorEvidence {
            provenance: "cockpit_prior_scan",
            last_scan_at,
            state: "unavailable_not_scanned",
            matched_feature_count: 0,
            omitted_features: 0,
            features: Vec::new(),
            linked_test_count: 0,
            omitted_tests: 0,
            linked_tests: Vec::new(),
            note: NOTE,
        });
        return;
    }

    let features = state.features.values().map(|view| view.feature.clone()).collect::<Vec<_>>();
    let matches = feature_path_matches(&features, changed);
    let matched_feature_count = matches.len();
    let mut selected_features = Vec::new();
    let mut linked = BTreeMap::<String, LinkedTestHint>::new();
    for (id, paths) in matches.iter().take(10) {
        let Some(view) = state.feature(id) else { continue };
        selected_features.push(FeatureHint {
            id: id.clone(),
            title: view.feature.title.clone(),
            importance: view.feature.importance.as_str(),
            matched_paths: paths.iter().take(3).cloned().collect(),
            linked_test_count: view.tests.len(),
            prior_health: view.health.state.as_str(),
        });
        for test_id in &view.tests {
            if let Some(test) = state.tests.get(test_id) {
                linked.entry(test_id.as_str().to_owned()).or_insert_with(|| LinkedTestHint {
                    id: test_id.as_str().to_owned(),
                    kind: test.test.kind.as_str(),
                    source_path: test.test.source_path.as_ref().map(|p| p.as_str().to_owned()),
                    prior_status: test.latest.as_ref().map(|r| r.status.as_str()),
                });
            }
        }
    }
    let linked_test_count = linked.len();
    plan.prior_evidence = Some(PriorEvidence {
        provenance: "cockpit_prior_scan",
        last_scan_at,
        state: "historical_not_verified_for_worktree",
        matched_feature_count,
        omitted_features: matched_feature_count.saturating_sub(selected_features.len()),
        features: selected_features,
        linked_test_count,
        omitted_tests: linked_test_count.saturating_sub(12),
        linked_tests: linked.into_values().take(12).collect(),
        note: NOTE,
    });
}

#[derive(Deserialize)]
struct MoonWorkspace {
    projects: BTreeMap<String, String>,
}

#[derive(Default)]
struct Languages {
    rust: bool,
    js: bool,
    path: String,
}

/// Explicit files are useful to agents that have an uncommitted edit transaction.
/// With no explicit files, compare the worktree (including staged changes) to
/// HEAD or the requested base and include untracked, non-ignored files.
pub fn changes(
    root: &Path,
    explicit: &[String],
    base: Option<&str>,
) -> Result<(Vec<String>, String)> {
    if !explicit.is_empty() {
        if base.is_some() {
            bail!("--base cannot be combined with --file");
        }
        let mut files = BTreeSet::new();
        for name in explicit {
            files.insert(normalize(name)?);
        }
        return Ok((files.into_iter().collect(), "explicit".into()));
    }
    let base = base.unwrap_or("HEAD");
    if base.starts_with('-') || base.is_empty() {
        bail!("--base must be a git revision, not a flag");
    }
    let tracked = git(root, &["diff", "--name-only", "-z", "--diff-filter=ACDMRTUXB", base, "--"])?;
    let untracked = git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    let mut files = BTreeSet::new();
    for bytes in [tracked, untracked] {
        for entry in bytes.split(|b| *b == 0).filter(|s| !s.is_empty()) {
            let name = std::str::from_utf8(entry).context("non-UTF-8 git path")?;
            files.insert(normalize(name)?);
        }
    }
    Ok((files.into_iter().collect(), format!("git:{base}+worktree")))
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .with_context(|| format!("running git in {}", root.display()))?;
    if !output.status.success() {
        bail!("git {} failed: {}", args[0], String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(output.stdout)
}

fn normalize(name: &str) -> Result<String> {
    let mut parts = Vec::new();
    for component in Path::new(name).components() {
        match component {
            Component::Normal(s) => parts.push(s.to_string_lossy().to_string()),
            Component::CurDir => {}
            _ => bail!("unsafe changed path: {name:?}"),
        }
    }
    if parts.is_empty() {
        bail!("empty changed path");
    }
    Ok(parts.join("/"))
}

fn read_moon(root: &Path) -> Result<Option<BTreeMap<String, String>>> {
    let path = root.join(".moon/workspace.yml");
    if !path.is_file() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let workspace: MoonWorkspace =
        serde_yaml_ng::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(workspace.projects))
}

fn owner<'a>(name: &str, projects: &'a BTreeMap<String, String>) -> Option<(&'a str, &'a str)> {
    projects
        .iter()
        .filter(|(_, dir)| {
            let dir = dir.trim_matches('/');
            !dir.is_empty() && (name == dir || name.starts_with(&format!("{dir}/")))
        })
        .max_by_key(|(_, dir)| dir.len())
        .map(|(id, dir)| (id.as_str(), dir.as_str()))
}

fn global_input(p: &str) -> bool {
    matches!(
        p,
        "Cargo.toml"
            | "Cargo.lock"
            | "package.json"
            | "bun.lock"
            | "bunfig.toml"
            | "tsconfig.base.json"
            | "rust-toolchain.toml"
            | "cockpit.toml"
            | ".mcp.json"
    ) || p.starts_with(".cargo/")
        || p.starts_with(".moon/")
        || p.starts_with(".github/")
        || p.starts_with("tooling/")
}

fn is_test(p: &str) -> bool {
    p.contains("/tests/")
        || p.starts_with("tests/")
        || p.contains("/test/")
        || p.ends_with("_test.rs")
        || p.contains(".test.")
        || p.contains(".spec.")
}

/// This planner is intentionally path-based. The impact of a changed public
/// symbol, generated code or build script is not proven by a project prefix.
pub fn analyze(
    root: &Path,
    files: &[String],
    source: String,
    mode: Mode,
    fast: FastBackend,
) -> Result<Plan> {
    analyze_with_policy(root, files, source, mode, fast, &DevelopmentSection::default())
}

/// Shares a checked-in verification policy between local agents and CI.
pub fn analyze_with_policy(
    root: &Path,
    files: &[String],
    source: String,
    mode: Mode,
    fast: FastBackend,
    policy: &DevelopmentSection,
) -> Result<Plan> {
    let moon = read_moon(root)?;
    let mut projects = BTreeMap::<String, Languages>::new();
    let mut unmapped = Vec::new();
    let mut unknown = Vec::new();
    let mut code_changes = false;
    let mut doc_changes = false;
    for original in files {
        let p = normalize(original)?;
        if global_input(&p) {
            unknown.push(p);
            continue;
        }
        let ext = Path::new(&p).extension().and_then(|x| x.to_str()).unwrap_or("");
        let rust = ext == "rs";
        let js = matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mts" | "cts" | "mjs" | "cjs");
        if ext == "md" || ext == "mdx" {
            doc_changes = true;
            continue;
        }
        if !rust && !js {
            unknown.push(p);
            continue;
        }
        code_changes = true;
        let mapping = if let Some(ref available) = moon {
            owner(&p, available).map(|(id, dir)| (id.to_owned(), dir.to_owned()))
        } else if root.join("Cargo.toml").is_file() || root.join("package.json").is_file() {
            Some(("root".into(), ".".into()))
        } else {
            None
        };
        if let Some((id, dir)) = mapping {
            let entry = projects.entry(id).or_default();
            entry.path = dir;
            entry.rust |= rust;
            entry.js |= js;
        } else {
            unmapped.push(p);
        }
    }
    // An unmatched source or a root build/config input can affect any project.
    let uncertain = !unmapped.is_empty() || !unknown.is_empty();
    let scope = if uncertain {
        "unknown"
    } else if projects.len() > 1 {
        "multiple_projects_path_based"
    } else if code_changes {
        "project_path_based"
    } else if doc_changes {
        "documentation_only_path_based"
    } else {
        "no_changes"
    };
    // In a clean CI checkout there are no uncommitted edits. The CI stage
    // must never interpret that as "nothing needs verification".
    if mode == Mode::Ci && files.is_empty() {
        if let Some(ref all) = moon {
            for (id, dir) in all {
                let rust = root.join(dir).join("Cargo.toml").is_file();
                let js = root.join(dir).join("package.json").is_file();
                if rust || js {
                    projects.insert(id.clone(), Languages { rust, js, path: dir.clone() });
                }
            }
        } else {
            let rust = root.join("Cargo.toml").is_file();
            let js = root.join("package.json").is_file();
            if rust || js {
                projects.insert("root".into(), Languages { rust, js, path: ".".into() });
            }
        }
    }
    let mut actions = Vec::new();
    if !uncertain {
        for lang in projects.values() {
            let selected = match fast {
                FastBackend::Auto => match policy.rust_edit_backend {
                    RustEditBackend::None => FastBackend::None,
                    RustEditBackend::Cargo => FastBackend::Cargo,
                    RustEditBackend::Blaze => FastBackend::Blaze,
                },
                explicit => explicit,
            };
            if mode == Mode::Edit && lang.rust && selected != FastBackend::None {
                let command = match selected {
                    FastBackend::Blaze => vec!["blaze".into(), "check".into()],
                    _ => vec!["cargo".into(), "check".into(), "--workspace".into()],
                };
                actions.push(Action {
                    stage: "edit",
                    cwd: lang.path.clone(),
                    command,
                    purpose: "Optional Rust edit-time diagnostics",
                    authority: "advisory_not_full_verification",
                    status: "recommended_not_run",
                });
            }
        }
        if mode == Mode::Checkpoint || mode == Mode::Ci {
            if mode == Mode::Ci && policy.rust_ci_backend == RustCiBackend::BlazeThenCargo {
                for lang in projects.values().filter(|lang| lang.rust) {
                    actions.push(Action {
                        stage: "ci",
                        cwd: lang.path.clone(),
                        command: vec!["blaze".into(), "check".into()],
                        purpose: "Optional fast Rust preflight before mandatory pinned Cargo",
                        authority: "advisory",
                        status: "recommended_not_run",
                    });
                }
            }
            for (id, lang) in &projects {
                for (enabled, kind) in [(lang.rust, "rust"), (lang.js, "js")] {
                    if !enabled {
                        continue;
                    }
                    let check = if moon.is_some() {
                        vec!["moon".into(), "run".into(), format!("{id}:{kind}-check")]
                    } else if kind == "rust" {
                        vec![
                            "cargo".into(),
                            "check".into(),
                            "--workspace".into(),
                            "--all-targets".into(),
                        ]
                    } else {
                        vec!["bun".into(), "run".into(), "typecheck".into()]
                    };
                    let test = if moon.is_some() {
                        vec!["moon".into(), "run".into(), format!("{id}:{kind}-test")]
                    } else if kind == "rust" {
                        vec!["cargo".into(), "test".into(), "--workspace".into()]
                    } else {
                        vec!["bun".into(), "run".into(), "test".into()]
                    };
                    let purpose = if files.iter().any(|p| is_test(p)) {
                        "Checkpoint changed tests and their owning project"
                    } else {
                        "Checkpoint owning project; dependency impact is not proven"
                    };
                    actions.push(Action {
                        stage: if mode == Mode::Ci { "ci" } else { "checkpoint" },
                        cwd: lang.path.clone(),
                        command: check,
                        purpose,
                        authority: if mode == Mode::Ci { "ci_check" } else { "local_check" },
                        status: "recommended_not_run",
                    });
                    actions.push(Action {
                        stage: if mode == Mode::Ci { "ci" } else { "checkpoint" },
                        cwd: lang.path.clone(),
                        command: test,
                        purpose,
                        authority: if mode == Mode::Ci {
                            "ci_test"
                        } else {
                            "local_test_not_full_ci"
                        },
                        status: "recommended_not_run",
                    });
                }
            }
        }
    }
    let mut all = BTreeSet::new();
    for p in files {
        all.insert(normalize(p)?);
    }
    let changed_count = all.len();
    let changed_files = all.iter().take(32).cloned().collect::<Vec<_>>();
    let omitted_changed_files = changed_count.saturating_sub(changed_files.len());
    Ok(Plan {
        mode: match mode {
            Mode::Edit => "edit",
            Mode::Checkpoint => "checkpoint",
            Mode::Ci => "ci",
        },
        source,
        changed_count,
        changed_files,
        omitted_changed_files,
        projects: projects.keys().cloned().collect(),
        unmapped_files: unmapped.into_iter().take(16).collect(),
        unknown_inputs: unknown.into_iter().take(16).collect(),
        scope,
        actions,
        authoritative_ci_required: changed_count != 0 || mode == Mode::Ci,
        verification_status: "not_run",
        limitations: vec![
            "Path ownership is not compiler-resolved change-impact analysis.",
            "No symbol, dependency, generated-output or exact test coverage analysis was performed.",
            "No command was executed; absence of a suggested check does not prove safety.",
            "Blaze uses its own Rust toolchain; project-pinned Cargo and existing CI remain authoritative.",
            "CI recommendations are not executed automatically; the existing repository workflow is the actual gate.",
        ],
        prior_evidence: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sample() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join(".moon")).unwrap();
        fs::write(t.path().join(".moon/workspace.yml"), "projects:\n  core: engine/core\n  sheet: modules/sheet\n  sheet-viewer: modules/sheet/viewer\n").unwrap();
        t
    }

    fn plan(root: &Path, paths: &[&str], mode: Mode, fast: FastBackend) -> Plan {
        analyze(
            root,
            &paths.iter().map(|x| x.to_string()).collect::<Vec<_>>(),
            "test".into(),
            mode,
            fast,
        )
        .unwrap()
    }

    #[test]
    fn git_changes_include_modified_and_untracked_files() {
        let t = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(t.path())
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        };
        run(&["init", "-q"]);
        fs::write(t.path().join("tracked.rs"), "fn old() {}\n").unwrap();
        run(&["add", "tracked.rs"]);
        run(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.org",
            "commit",
            "-q",
            "-m",
            "initial",
        ]);
        fs::write(t.path().join("tracked.rs"), "fn new() {}\n").unwrap();
        fs::write(t.path().join("untracked.ts"), "export const n = 1;\n").unwrap();
        let (files, source) = changes(t.path(), &[], None).unwrap();
        assert_eq!(files, vec!["tracked.rs", "untracked.ts"]);
        assert_eq!(source, "git:HEAD+worktree");
    }

    #[test]
    fn single_cargo_project_has_root_fallback() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("Cargo.toml"), "[package]\nname = \"example\"\n").unwrap();
        let p = plan(t.path(), &["src/lib.rs"], Mode::Checkpoint, FastBackend::None);
        assert_eq!(p.projects, vec!["root"]);
        assert_eq!(p.actions[0].command, vec!["cargo", "check", "--workspace", "--all-targets"]);
        assert_eq!(p.actions[1].command, vec!["cargo", "test", "--workspace"]);
    }

    #[test]
    fn feature_hints_match_existing_ownership_globs() {
        let make_feature = |id: &str, patterns: &[&str]| Feature {
            id: crate::ids::FeatureId::new(id),
            title: id.into(),
            description: None,
            lifecycle: crate::model::Lifecycle::Stable,
            importance: crate::model::Importance::Normal,
            owners: vec![],
            tags: vec![],
            paths: patterns.iter().map(|p| (*p).into()).collect(),
            adr_ids: vec![],
            dependencies: vec![],
            aliases: vec![],
            claims: vec![],
            acceptance_criteria: vec![],
            sections: vec![],
            body_md: String::new(),
            since: None,
            superseded_by: None,
            references: vec![],
            group: vec![],
            source: RelPath::new("docs/features/test.md"),
            extra: Default::default(),
        };
        let features =
            vec![make_feature("A", &["modules/doc/**"]), make_feature("B", &["modules/sheet/**"])];
        let matches = feature_path_matches(
            &features,
            &["modules/sheet/src/lib.rs".into(), "modules/doc/src/lib.rs".into()],
        );
        assert_eq!(matches.len(), 2);
        assert_eq!(matches["B"], vec!["modules/sheet/src/lib.rs"]);
    }

    #[test]
    fn optional_prior_evidence_never_marks_plan_verified() {
        let t = sample();
        let mut p = plan(t.path(), &["engine/core/src/lib.rs"], Mode::Edit, FastBackend::None);
        let state = ProjectState::empty(crate::model::Project {
            id: "sample".into(),
            name: "Sample".into(),
            root: t.path().to_path_buf(),
            repos: vec![],
            components: vec![],
            primary_branch: "main".into(),
        });
        attach_prior_evidence(&mut p, &state, &["engine/core/src/lib.rs".into()]);
        assert_eq!(p.verification_status, "not_run");
        assert_eq!(p.prior_evidence.as_ref().unwrap().state, "unavailable_not_scanned");
    }

    #[test]
    fn unsafe_paths_rejected() {
        assert!(normalize("../secret").is_err());
        assert!(normalize("/tmp/secret").is_err());
        assert!(normalize("./src/lib.rs").is_ok());
    }

    #[test]
    fn docs_do_not_trigger_builds() {
        let t = sample();
        let p = plan(t.path(), &["docs/README.md"], Mode::Checkpoint, FastBackend::None);
        assert_eq!(p.scope, "documentation_only_path_based");
        assert!(p.actions.is_empty());
        assert!(p.authoritative_ci_required);
    }

    #[test]
    fn nested_project_ownership_wins() {
        let t = sample();
        let p = plan(
            t.path(),
            &["modules/sheet/viewer/src/lib.ts"],
            Mode::Checkpoint,
            FastBackend::None,
        );
        assert_eq!(p.projects, vec!["sheet-viewer"]);
        assert_eq!(p.actions.len(), 2);
        assert_eq!(p.actions[0].command[2], "sheet-viewer:js-check");
    }

    #[test]
    fn root_config_forces_unknown_scope() {
        let t = sample();
        let p = plan(
            t.path(),
            &["bun.lock", "modules/sheet/src/lib.rs"],
            Mode::Checkpoint,
            FastBackend::None,
        );
        assert_eq!(p.scope, "unknown");
        assert!(p.actions.is_empty());
        assert_eq!(p.projects, vec!["sheet"]);
    }

    #[test]
    fn blaze_is_opt_in_and_advisory() {
        let t = sample();
        let files = &["engine/core/src/lib.rs"];
        assert!(plan(t.path(), files, Mode::Edit, FastBackend::None).actions.is_empty());
        let p = plan(t.path(), files, Mode::Edit, FastBackend::Blaze);
        assert_eq!(p.actions[0].command, vec!["blaze", "check"]);
        assert!(p.actions[0].authority.starts_with("advisory"));
    }

    #[test]
    fn ci_mode_never_claims_to_run_ci() {
        let t = sample();
        let p = plan(t.path(), &["engine/core/src/lib.rs"], Mode::Ci, FastBackend::None);
        assert!(!p.actions.is_empty());
        assert!(p.actions.iter().all(|action| action.stage == "ci"));
        assert_eq!(p.verification_status, "not_run");
        assert!(p.authoritative_ci_required);
    }

    #[test]
    fn empty_change_set_is_not_verified_success() {
        let t = sample();
        let p = plan(t.path(), &[], Mode::Edit, FastBackend::None);
        assert_eq!(p.scope, "no_changes");
        assert!(!p.authoritative_ci_required);
        assert_eq!(p.verification_status, "not_run");
    }
}
