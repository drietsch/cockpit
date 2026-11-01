//! Conservative, read-only change-impact and verification planning.
//!
//! A plan is not test evidence. It neither executes commands nor asserts that
//! selected checks cover the entire dependency graph. Existing CI is the gate.

use crate::agent_workspace::{MoonProject, MoonProjects, read_moon};
use crate::config::{DevelopmentSection, Downstream, RustCiBackend, RustEditBackend};
use crate::link::PathOwnership;
use crate::model::Feature;
use crate::paths::RelPath;
use crate::state::ProjectState;
use anyhow::{Context, Result, bail};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Serialize;
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
    /// Projects that own a changed path (or, for a root lockfile, read it).
    pub projects: Vec<String>,
    /// `none`, `direct_moon_depends_on`, `deep_moon_depends_on` or `unavailable_no_moon_workspace`.
    pub downstream: &'static str,
    /// Dependents added by downstream expansion; their checks are in `actions` too.
    pub downstream_projects: Vec<String>,
    pub path_classes: PathClasses,
    pub unmapped_files: Vec<String>,
    pub unknown_inputs: Vec<String>,
    /// Why a file stayed unmapped, where known (bounded).
    pub notes: Vec<String>,
    pub scope: &'static str,
    pub actions: Vec<Action>,
    pub authoritative_ci_required: bool,
    pub verification_status: &'static str,
    pub limitations: Vec<&'static str>,
    /// Optional bounded hints from a previous Cockpit scan, never verification of this edit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prior_evidence: Option<PriorEvidence>,
    /// Optional bounded tests selected from prior feature links.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_selection: Option<crate::agent_tests::TestSelection>,
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

/// Explicit files are useful to agents that have an uncommitted edit transaction.
/// With no explicit files, compare the worktree (including staged changes) to
/// HEAD and include untracked, non-ignored files. With `base`, the comparison
/// starts at `git merge-base <base> HEAD` (the three-dot semantics of a pull
/// request): what the base gained after the branch forked is not this change.
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
    let (from, source) = match base {
        None => ("HEAD".to_owned(), "git:HEAD+worktree".to_owned()),
        Some(base) => {
            if base.starts_with('-') || base.is_empty() {
                bail!("--base must be a git revision, not a flag");
            }
            let merge_base = git(root, &["merge-base", base, "HEAD"])
                .with_context(|| format!("no merge base of {base} and HEAD"))?;
            let merge_base = String::from_utf8_lossy(&merge_base).trim().to_owned();
            let short = merge_base.get(..12).unwrap_or(&merge_base).to_owned();
            (merge_base, format!("git:merge-base({base})={short}+worktree"))
        }
    };
    let tracked =
        git(root, &["diff", "--name-only", "-z", "--diff-filter=ACDMRTUXB", &from, "--"])?;
    let untracked = git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    let mut files = BTreeSet::new();
    for bytes in [tracked, untracked] {
        for entry in bytes.split(|b| *b == 0).filter(|s| !s.is_empty()) {
            let name = std::str::from_utf8(entry).context("non-UTF-8 git path")?;
            files.insert(normalize(name)?);
        }
    }
    Ok((files.into_iter().collect(), source))
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

fn owner<'a>(name: &str, projects: &'a MoonProjects) -> Option<(&'a str, &'a MoonProject)> {
    projects
        .iter()
        .filter(|(_, project)| {
            let dir = project.dir.trim_matches('/');
            !dir.is_empty() && dir != "." && (name == dir || name.starts_with(&format!("{dir}/")))
        })
        .max_by_key(|(_, project)| project.dir.len())
        .map(|(id, project)| (id.as_str(), project))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lang {
    Rust,
    Js,
}

/// A root input that every project of one language reads (a root lockfile or manifest).
/// With a known project set it widens the plan to those projects instead of making it unknown.
fn workspace_input(p: &str) -> Option<Lang> {
    match p {
        "Cargo.toml" | "Cargo.lock" | "rust-toolchain.toml" | "rust-toolchain" => Some(Lang::Rust),
        _ if p.starts_with(".cargo/") => Some(Lang::Rust),
        "package.json" | "bun.lock" | "bun.lockb" | "bunfig.toml" | "tsconfig.base.json"
        | "tsconfig.json" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" => Some(Lang::Js),
        _ => None,
    }
}

fn global_input(p: &str) -> bool {
    workspace_input(p).is_some()
        || matches!(p, "cockpit.toml" | ".mcp.json")
        || p.starts_with(".moon/")
        || p.starts_with(".github/")
        || p.starts_with("tooling/")
}

fn file_name(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// The language a changed file is source or build input of, by name.
fn source_language(p: &str) -> Option<Lang> {
    let name = file_name(p);
    if matches!(name, "Cargo.toml" | "Cargo.lock") || name.starts_with("rust-toolchain") {
        return Some(Lang::Rust);
    }
    if name == "package.json" || (name.starts_with("tsconfig") && name.ends_with(".json")) {
        return Some(Lang::Js);
    }
    match Path::new(p).extension().and_then(|x| x.to_str()).unwrap_or("") {
        "rs" => Some(Lang::Rust),
        "ts" | "tsx" | "js" | "jsx" | "mts" | "cts" | "mjs" | "cjs" => Some(Lang::Js),
        _ => None,
    }
}

fn is_test(p: &str) -> bool {
    p.contains("/tests/")
        || p.starts_with("tests/")
        || p.contains("/test/")
        || p.ends_with("_test.rs")
        || p.contains(".test.")
        || p.contains(".spec.")
}

fn globs(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .with_context(|| format!("invalid [development] glob {pattern:?}"))?;
        builder.add(glob);
    }
    Ok(builder.build()?)
}

#[derive(Clone, Copy, Debug, Default)]
struct Need {
    check: bool,
    test: bool,
}

impl Need {
    const ALL: Need = Need { check: true, test: true };
    const TEST: Need = Need { check: false, test: true };
    fn add(&mut self, other: Need) {
        self.check |= other.check;
        self.test |= other.test;
    }
    fn any(self) -> bool {
        self.check || self.test
    }
}

/// Why a project is in the plan, strongest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Reason {
    Owner,
    Generated,
    SourceConfig,
    WorkspaceInput,
    Downstream,
    TestData,
    Ci,
}

impl Reason {
    fn purpose(self, tests_changed: bool) -> &'static str {
        match self {
            Reason::Owner if tests_changed => "Checkpoint changed tests and their owning project",
            Reason::Owner => "Checkpoint owning project; dependency impact is not proven",
            Reason::Generated => "Changed generated file: owning project's checks and tests",
            Reason::SourceConfig => {
                "Changed project configuration: owning project's checks and tests"
            }
            Reason::WorkspaceInput => {
                "Changed root lockfile or manifest: every project of that language"
            }
            Reason::Downstream => {
                "Dependent of a changed project (Moon dependsOn); declared edges, not compiler-resolved"
            }
            Reason::TestData => "Changed test data: owning project's tests",
            Reason::Ci => "CI: every discoverable project",
        }
    }
}

struct Planned {
    path: String,
    /// What the project can run.
    has_rust: bool,
    has_js: bool,
    rust: Need,
    js: Need,
    reason: Reason,
}

impl Planned {
    fn new(path: &str, has_rust: bool, has_js: bool, reason: Reason) -> Planned {
        Planned {
            path: path.to_owned(),
            has_rust,
            has_js,
            rust: Need::default(),
            js: Need::default(),
            reason,
        }
    }
    fn need(&mut self, lang: Option<Lang>, need: Need, reason: Reason) {
        match lang {
            Some(Lang::Rust) => self.rust.add(need),
            Some(Lang::Js) => self.js.add(need),
            None => {
                if self.has_rust {
                    self.rust.add(need);
                }
                if self.has_js {
                    self.js.add(need);
                }
            }
        }
        self.reason = self.reason.min(reason);
    }
}

/// Where a file belongs when the repository has no Moon workspace: the Cargo workspace that
/// builds it, else a root Bun/npm package.
fn fallback_owner(root: &Path, p: &str) -> Option<(String, String, bool, bool)> {
    let dir = match crate::agent_workspace::cargo_owner(root, p) {
        Some(owner) => owner.workspace_dir,
        None if root.join("package.json").is_file() => ".".to_owned(),
        None => return None,
    };
    let id = if dir == "." { "root".to_owned() } else { dir.clone() };
    let has_rust = root.join(&dir).join("Cargo.toml").is_file();
    let has_js = root.join(&dir).join("package.json").is_file();
    Some((id, dir, has_rust, has_js))
}

#[derive(Debug, Default, Serialize)]
pub struct PathClasses {
    pub test_data: usize,
    pub generated: usize,
    /// Project configuration (`source_config`) that planned its owning project like source.
    pub source_config: usize,
    pub documentation: usize,
    /// Root lockfiles/manifests that widened the plan to every project of a language.
    pub workspace_inputs: Vec<String>,
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
    let test_data = globs(&policy.test_data)?;
    let generated = globs(&policy.generated)?;
    let source_config = globs(&policy.source_config)?;
    let documentation = globs(&policy.documentation)?;
    let test_inputs = policy
        .test_inputs
        .iter()
        .map(|(pattern, ids)| Ok((globs(std::slice::from_ref(pattern))?, ids)))
        .collect::<Result<Vec<_>>>()?;
    let mut planned = BTreeMap::<String, Planned>::new();
    let mut unmapped = Vec::new();
    let mut unknown = Vec::new();
    let mut notes = Vec::<String>::new();
    let mut classes = PathClasses::default();
    let mut code_changes = false;
    let mut doc_changes = false;
    // Rust edit feedback: Cargo workspace directory -> packages (None = the whole workspace).
    let mut crates = BTreeMap::<String, (BTreeSet<String>, bool, bool)>::new();
    for original in files {
        let p = normalize(original)?;
        if let (Some(lang), Some(projects)) = (workspace_input(&p), moon.as_ref()) {
            classes.workspace_inputs.push(p.clone());
            code_changes = true;
            for (id, project) in projects {
                let has = if lang == Lang::Rust { project.rust } else { project.js };
                if has {
                    planned
                        .entry(id.clone())
                        .or_insert_with(|| {
                            Planned::new(
                                &project.dir,
                                project.rust,
                                project.js,
                                Reason::WorkspaceInput,
                            )
                        })
                        .need(Some(lang), Need::ALL, Reason::WorkspaceInput);
                }
            }
            continue;
        }
        if global_input(&p) {
            unknown.push(p);
            continue;
        }
        let ext = Path::new(&p).extension().and_then(|x| x.to_str()).unwrap_or("");
        if ext == "md" || ext == "mdx" || documentation.is_match(&p) {
            doc_changes = true;
            classes.documentation += 1;
            continue;
        }
        let consumers: Vec<&String> =
            test_inputs.iter().filter(|(g, _)| g.is_match(&p)).flat_map(|(_, ids)| *ids).collect();
        if !consumers.is_empty() {
            let mut mapped = true;
            for id in consumers {
                let project = moon.as_ref().and_then(|projects| projects.get(id));
                match project.filter(|project| project.rust || project.js) {
                    Some(project) => {
                        planned
                            .entry(id.clone())
                            .or_insert_with(|| {
                                Planned::new(
                                    &project.dir,
                                    project.rust,
                                    project.js,
                                    Reason::TestData,
                                )
                            })
                            .need(None, Need::TEST, Reason::TestData);
                    }
                    None => {
                        mapped = false;
                        if notes.len() < 16 {
                            notes.push(format!("{p}: test_inputs names unknown project {id}"));
                        }
                    }
                }
            }
            if mapped {
                classes.test_data += 1;
            } else {
                unmapped.push(p);
            }
            continue;
        }
        let class = if test_data.is_match(&p) {
            Some(Reason::TestData)
        } else if generated.is_match(&p) {
            Some(Reason::Generated)
        } else if source_config.is_match(&p) {
            Some(Reason::SourceConfig)
        } else {
            None
        };
        let lang = source_language(&p);
        if class.is_none() && lang.is_none() {
            unknown.push(p);
            continue;
        }
        let owning = if let Some(ref projects) = moon {
            owner(&p, projects).map(|(id, project)| {
                if project.languages_from == "unknown" {
                    // Nothing declares the project's languages: trust the source extension.
                    let (rust, js) = (lang == Some(Lang::Rust), lang == Some(Lang::Js));
                    (id.to_owned(), project.dir.clone(), rust, js)
                } else {
                    (id.to_owned(), project.dir.clone(), project.rust, project.js)
                }
            })
        } else {
            fallback_owner(root, &p)
        };
        let Some((id, dir, has_rust, has_js)) = owning else {
            unmapped.push(p);
            continue;
        };
        match class {
            Some(_) if !has_rust && !has_js => {
                if notes.len() < 16 {
                    notes.push(format!("{p}: owning project {id} declares no Rust or JS tasks"));
                }
                unmapped.push(p);
            }
            Some(reason) => {
                match reason {
                    Reason::TestData => classes.test_data += 1,
                    Reason::SourceConfig => {
                        classes.source_config += 1;
                        code_changes = true;
                    }
                    _ => {
                        classes.generated += 1;
                        code_changes = true;
                    }
                }
                let need = if reason == Reason::TestData { Need::TEST } else { Need::ALL };
                // A configuration file of one language (tsconfig) plans only that language,
                // when the project has it; otherwise every language the project has.
                let only = match (reason, lang) {
                    (Reason::SourceConfig, Some(Lang::Rust)) if has_rust => lang,
                    (Reason::SourceConfig, Some(Lang::Js)) if has_js => lang,
                    _ => None,
                };
                planned
                    .entry(id)
                    .or_insert_with(|| Planned::new(&dir, has_rust, has_js, reason))
                    .need(only, need, reason);
            }
            None => {
                let lang = lang.expect("classified above");
                let has = if lang == Lang::Rust { has_rust } else { has_js };
                if !has {
                    if notes.len() < 16 {
                        let kind = if lang == Lang::Rust { "Rust" } else { "JS/TS" };
                        notes.push(format!("{p}: owning project {id} has no {kind} tasks"));
                    }
                    unmapped.push(p);
                    continue;
                }
                code_changes = true;
                planned
                    .entry(id)
                    .or_insert_with(|| Planned::new(&dir, has_rust, has_js, Reason::Owner))
                    .need(Some(lang), Need::ALL, Reason::Owner);
                if lang == Lang::Rust {
                    if let Some(cargo) = crate::agent_workspace::cargo_owner(root, &p) {
                        let entry = crates.entry(cargo.workspace_dir).or_default();
                        match cargo.package {
                            Some(package) => {
                                entry.0.insert(package);
                            }
                            None => entry.1 = true,
                        }
                        entry.2 |= is_test(&p);
                    }
                }
            }
        }
    }
    // An unmatched source or a root build/config input can affect any project.
    let uncertain = !unmapped.is_empty() || !unknown.is_empty();
    let owning_projects: Vec<String> = planned.keys().cloned().collect();
    let mut downstream_projects = Vec::new();
    let downstream = match (&moon, policy.downstream) {
        (_, Downstream::None) => "none",
        (None, _) => "unavailable_no_moon_workspace",
        (Some(projects), depth) => {
            // Expand from changed code, generated files and project configuration only: test data reaches only its own
            // project's tests, and a root lockfile already lists every project it touches (a
            // project listed for it alone still gets its full checks as a dependent).
            let sources: BTreeSet<String> = planned
                .iter()
                .filter(|(_, p)| {
                    matches!(p.reason, Reason::Owner | Reason::Generated | Reason::SourceConfig)
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in
                crate::agent_workspace::dependents(projects, &sources, depth == Downstream::Deep)
            {
                let project = &projects[&id];
                if !(project.rust || project.js) {
                    continue;
                }
                let entry = planned.entry(id.clone()).or_insert_with(|| {
                    Planned::new(&project.dir, project.rust, project.js, Reason::Downstream)
                });
                entry.need(None, Need::ALL, Reason::Downstream);
                if !owning_projects.contains(&id) {
                    downstream_projects.push(id);
                }
            }
            if depth == Downstream::Deep {
                "deep_moon_depends_on"
            } else {
                "direct_moon_depends_on"
            }
        }
    };
    let scope = if uncertain {
        "unknown"
    } else if !classes.workspace_inputs.is_empty() {
        "workspace_wide_path_based"
    } else if planned.len() > 1 {
        "multiple_projects_path_based"
    } else if !planned.is_empty() || code_changes {
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
            for (id, project) in all {
                if project.rust || project.js {
                    planned
                        .entry(id.clone())
                        .or_insert_with(|| {
                            Planned::new(&project.dir, project.rust, project.js, Reason::Ci)
                        })
                        .need(None, Need::ALL, Reason::Ci);
                }
            }
        } else {
            let rust = root.join("Cargo.toml").is_file();
            let js = root.join("package.json").is_file();
            if rust || js {
                planned
                    .entry("root".into())
                    .or_insert_with(|| Planned::new(".", rust, js, Reason::Ci))
                    .need(None, Need::ALL, Reason::Ci);
            }
        }
    }
    let tests_changed = files.iter().any(|p| is_test(p));
    let mut actions = Vec::new();
    let selected = match fast {
        FastBackend::Auto => match policy.rust_edit_backend {
            RustEditBackend::None => FastBackend::None,
            RustEditBackend::Cargo => FastBackend::Cargo,
            RustEditBackend::Blaze => FastBackend::Blaze,
        },
        explicit => explicit,
    };
    // Edit-time diagnostics are advisory and scoped to the changed crates, in the Cargo
    // workspace that builds them. They never claim the change is verified, so an unknown
    // scope does not withhold them (it still withholds checkpoint and CI suggestions).
    if mode == Mode::Edit && selected != FastBackend::None {
        for (dir, (packages, whole, tests)) in &crates {
            let mut command: Vec<String> = match selected {
                FastBackend::Blaze => vec!["blaze".into(), "check".into()],
                _ => vec!["cargo".into(), "check".into()],
            };
            if *whole || packages.is_empty() {
                command.push("--workspace".into());
            } else {
                for package in packages {
                    command.push("-p".into());
                    command.push(package.clone());
                }
            }
            if *tests {
                command.push("--all-targets".into());
            }
            actions.push(Action {
                stage: "edit",
                cwd: dir.clone(),
                command,
                purpose: "Optional Rust edit-time diagnostics for the changed crates",
                authority: "advisory_not_full_verification",
                status: "recommended_not_run",
            });
        }
    }
    if !uncertain && (mode == Mode::Checkpoint || mode == Mode::Ci) {
        if mode == Mode::Ci && policy.rust_ci_backend == RustCiBackend::BlazeThenCargo {
            for project in planned.values().filter(|p| p.rust.check) {
                actions.push(Action {
                    stage: "ci",
                    cwd: project.path.clone(),
                    command: vec!["blaze".into(), "check".into()],
                    purpose: "Optional fast Rust preflight before mandatory pinned Cargo",
                    authority: "advisory",
                    status: "recommended_not_run",
                });
            }
        }
        for (id, project) in &planned {
            for (need, kind) in [(project.rust, "rust"), (project.js, "js")] {
                if !need.any() {
                    continue;
                }
                let purpose = project.reason.purpose(tests_changed);
                let stage = if mode == Mode::Ci { "ci" } else { "checkpoint" };
                if need.check {
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
                    actions.push(Action {
                        stage,
                        cwd: project.path.clone(),
                        command: check,
                        purpose,
                        authority: if mode == Mode::Ci { "ci_check" } else { "local_check" },
                        status: "recommended_not_run",
                    });
                }
                if need.test {
                    let test = if moon.is_some() {
                        vec!["moon".into(), "run".into(), format!("{id}:{kind}-test")]
                    } else if kind == "rust" {
                        vec!["cargo".into(), "test".into(), "--workspace".into()]
                    } else {
                        vec!["bun".into(), "run".into(), "test".into()]
                    };
                    actions.push(Action {
                        stage,
                        cwd: project.path.clone(),
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
    classes.workspace_inputs.truncate(16);
    let projects = if mode == Mode::Ci && files.is_empty() {
        planned.keys().cloned().collect()
    } else {
        owning_projects
    };
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
        projects,
        downstream,
        downstream_projects,
        path_classes: classes,
        unmapped_files: unmapped.into_iter().take(16).collect(),
        unknown_inputs: unknown.into_iter().take(16).collect(),
        notes,
        scope,
        actions,
        authoritative_ci_required: changed_count != 0 || mode == Mode::Ci,
        verification_status: "not_run",
        limitations: vec![
            "Path ownership is not compiler-resolved change-impact analysis.",
            "Downstream projects, when listed, come from declared Moon dependsOn edges, not a resolved build graph.",
            "Test-data and generated-file classes come from configured globs; an unmatched non-source file keeps the scope unknown.",
            "No command was executed; absence of a suggested check does not prove safety.",
            "Blaze uses its own Rust toolchain; project-pinned Cargo and existing CI remain authoritative.",
            "CI recommendations are not executed automatically; the existing repository workflow is the actual gate.",
        ],
        prior_evidence: None,
        test_selection: None,
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

    /// A paged-like monorepo: per-project Cargo workspaces, Moon tags and dependsOn.
    fn monorepo() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        write(
            r,
            ".moon/workspace.yml",
            "projects:\n  core: engine/core\n  image: modules/image\n  editor: apps/editor\n  tests: tests\n",
        );
        write(r, "engine/core/moon.yml", "language: rust\ntags: [rust]\n");
        write(r, "engine/core/package.json", "{}");
        write(r, "engine/core/Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
        write(r, "engine/core/crates/geom/Cargo.toml", "[package]\nname = \"paged-geom\"\n");
        write(r, "engine/core/crates/wire/Cargo.toml", "[package]\nname = \"paged-wire\"\n");
        write(
            r,
            "modules/image/moon.yml",
            "tags: [rust, node]\ndependsOn:\n  - id: core\n    scope: production\n",
        );
        write(r, "modules/image/Cargo.toml", "[workspace]\nmembers = [\"image-psd\"]\n");
        write(r, "modules/image/image-psd/Cargo.toml", "[package]\nname = \"image-psd\"\n");
        write(
            r,
            "apps/editor/moon.yml",
            "language: typescript\ntags: [node]\ndependsOn: [core, image]\n",
        );
        write(r, "tests/moon.yml", "tags: [node]\ndependsOn:\n  - editor\n");
        t
    }

    fn plan_with(root: &Path, paths: &[&str], mode: Mode, policy: &DevelopmentSection) -> Plan {
        analyze_with_policy(
            root,
            &paths.iter().map(|x| x.to_string()).collect::<Vec<_>>(),
            "test".into(),
            mode,
            FastBackend::Auto,
            policy,
        )
        .unwrap()
    }

    fn commands(p: &Plan) -> Vec<String> {
        p.actions.iter().map(|a| format!("({}) {}", a.cwd, a.command.join(" "))).collect()
    }

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
            &[".github/workflows/ci.yml", "modules/sheet/src/lib.rs"],
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
        write(t.path(), "engine/core/Cargo.toml", "[package]\nname = \"core\"\n[workspace]\n");
        let files = &["engine/core/src/lib.rs"];
        assert!(plan(t.path(), files, Mode::Edit, FastBackend::None).actions.is_empty());
        let p = plan(t.path(), files, Mode::Edit, FastBackend::Blaze);
        assert_eq!(p.actions[0].command, vec!["blaze", "check", "-p", "core"]);
        assert_eq!(p.actions[0].cwd, "engine/core");
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

    #[test]
    fn base_diffs_from_the_merge_base() {
        let t = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(t.path())
                .args(["-c", "user.name=Test", "-c", "user.email=test@example.org"])
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        };
        run(&["init", "-q", "-b", "main"]);
        fs::write(t.path().join("shared.rs"), "fn a() {}\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "fork point"]);
        run(&["checkout", "-q", "-b", "topic"]);
        fs::write(t.path().join("mine.rs"), "fn mine() {}\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "topic work"]);
        run(&["checkout", "-q", "main"]);
        fs::write(t.path().join("theirs.rs"), "fn theirs() {}\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "main moved on"]);
        run(&["checkout", "-q", "topic"]);
        let (files, source) = changes(t.path(), &[], Some("main")).unwrap();
        // theirs.rs is what main gained after the fork, not this branch's change.
        assert_eq!(files, vec!["mine.rs"]);
        assert!(source.starts_with("git:merge-base(main)="), "{source}");
        assert!(changes(t.path(), &[], Some("no-such-ref")).is_err());
    }

    #[test]
    fn languages_come_from_moon_tags_not_extensions() {
        let t = monorepo();
        let policy = DevelopmentSection::default();
        // core has a package.json, but Moon gives it only the rust-* tasks.
        let p =
            plan_with(t.path(), &["engine/core/packages/wasm/index.ts"], Mode::Checkpoint, &policy);
        assert_eq!(p.scope, "unknown");
        assert_eq!(p.unmapped_files, vec!["engine/core/packages/wasm/index.ts"]);
        assert!(p.notes[0].contains("core has no JS/TS tasks"), "{:?}", p.notes);
        let p = plan_with(t.path(), &["modules/image/glue/src/a.ts"], Mode::Checkpoint, &policy);
        assert_eq!(
            commands(&p),
            vec![
                "(modules/image) moon run image:js-check",
                "(modules/image) moon run image:js-test"
            ]
        );
    }

    #[test]
    fn test_data_runs_the_owning_projects_tests_only() {
        let t = monorepo();
        let policy = DevelopmentSection { downstream: Downstream::Deep, ..Default::default() };
        let p = plan_with(
            t.path(),
            &[
                "modules/image/conformance/fixtures/gray-8.psd",
                "modules/image/conformance/fixtures/x.json",
            ],
            Mode::Checkpoint,
            &policy,
        );
        assert_eq!(p.scope, "project_path_based");
        assert_eq!(p.path_classes.test_data, 2);
        assert!(p.downstream_projects.is_empty(), "test data does not reach dependents");
        assert_eq!(
            commands(&p),
            vec![
                "(modules/image) moon run image:rust-test",
                "(modules/image) moon run image:js-test"
            ]
        );
        // The same kind of file outside a test-data glob stays unknown: fail closed.
        let p =
            plan_with(t.path(), &["modules/image/assets/gray-8.psd"], Mode::Checkpoint, &policy);
        assert_eq!(p.scope, "unknown");
        assert!(p.actions.is_empty());
    }

    #[test]
    fn configured_generated_and_documentation_globs() {
        let t = monorepo();
        let policy = DevelopmentSection {
            generated: vec!["apps/editor/**/feature-ids.json".into(), "**/locales/*.json".into()],
            documentation: vec!["docs/**/*.svg".into()],
            ..Default::default()
        };
        let p = plan_with(
            t.path(),
            &[
                "apps/editor/apps/canvas/feature-ids.json",
                "modules/image/glue/locales/de.json",
                "docs/development/diagram.svg",
            ],
            Mode::Checkpoint,
            &policy,
        );
        assert_eq!(p.scope, "multiple_projects_path_based");
        assert_eq!((p.path_classes.generated, p.path_classes.documentation), (2, 1));
        assert_eq!(
            commands(&p),
            vec![
                "(apps/editor) moon run editor:js-check",
                "(apps/editor) moon run editor:js-test",
                "(modules/image) moon run image:rust-check",
                "(modules/image) moon run image:rust-test",
                "(modules/image) moon run image:js-check",
                "(modules/image) moon run image:js-test",
            ]
        );
    }

    #[test]
    fn source_config_plans_its_owning_project_like_source() {
        let t = monorepo();
        let policy = DevelopmentSection { downstream: Downstream::Deep, ..Default::default() };
        let p = plan_with(
            t.path(),
            &["modules/image/packages/bundle/manifest.json"],
            Mode::Checkpoint,
            &policy,
        );
        assert_eq!(p.scope, "multiple_projects_path_based");
        assert_eq!(p.path_classes.source_config, 1);
        assert_eq!(p.downstream_projects, vec!["editor", "tests"]);
        let c = commands(&p);
        assert!(c.contains(&"(modules/image) moon run image:rust-check".to_string()), "{c:?}");
        assert!(c.contains(&"(modules/image) moon run image:js-test".to_string()), "{c:?}");
        // moon.yml of a project is configuration too.
        let p = plan_with(t.path(), &["apps/editor/moon.yml"], Mode::Checkpoint, &policy);
        assert_eq!((p.scope, p.path_classes.source_config), ("multiple_projects_path_based", 1));
        // Test data wins over configuration inside a fixtures directory.
        let p = plan_with(
            t.path(),
            &["modules/image/conformance/fixtures/manifest.json"],
            Mode::Checkpoint,
            &policy,
        );
        assert_eq!((p.path_classes.test_data, p.path_classes.source_config), (1, 0));
    }

    #[test]
    fn source_config_never_covers_root_configuration_or_unowned_files() {
        let t = monorepo();
        let policy = DevelopmentSection::default();
        for file in [
            ".github/workflows/manifest.json",
            ".moon/manifest.json",
            "tooling/release/manifest.json",
            "manifest.json",
            "moon.yml",
            "docs/manifest.json",
        ] {
            let p = plan_with(t.path(), &[file], Mode::Checkpoint, &policy);
            assert_eq!(p.scope, "unknown", "{file}");
            assert!(p.actions.is_empty(), "{file}");
            assert_eq!(p.path_classes.source_config, 0, "{file}");
        }
    }

    #[test]
    fn source_config_globs_replace_the_defaults() {
        let t = monorepo();
        let file = "modules/image/packages/bundle/manifest.json";
        let policy = DevelopmentSection { source_config: vec![], ..Default::default() };
        assert_eq!(plan_with(t.path(), &[file], Mode::Checkpoint, &policy).scope, "unknown");
        let policy = DevelopmentSection {
            source_config: vec!["**/*.settings.json".into()],
            ..Default::default()
        };
        assert_eq!(plan_with(t.path(), &[file], Mode::Checkpoint, &policy).scope, "unknown");
        let p = plan_with(t.path(), &["apps/editor/app.settings.json"], Mode::Checkpoint, &policy);
        assert_eq!(
            commands(&p),
            vec!["(apps/editor) moon run editor:js-check", "(apps/editor) moon run editor:js-test"]
        );
    }

    #[test]
    fn test_inputs_reach_the_named_projects_tests() {
        let t = monorepo();
        let mut policy = DevelopmentSection::default();
        let file = "docs/development/matrix.json";
        // Outside every project and unconfigured: unknown.
        assert_eq!(plan_with(t.path(), &[file], Mode::Checkpoint, &policy).scope, "unknown");
        policy.test_inputs.insert("docs/development/*.json".into(), vec!["editor".into()]);
        let p = plan_with(t.path(), &[file], Mode::Checkpoint, &policy);
        assert_eq!(p.scope, "project_path_based");
        assert_eq!(commands(&p), vec!["(apps/editor) moon run editor:js-test"]);
        policy.test_inputs.insert("docs/development/*.json".into(), vec!["nope".into()]);
        let p = plan_with(t.path(), &[file], Mode::Checkpoint, &policy);
        assert_eq!(p.scope, "unknown");
        assert!(p.notes[0].contains("unknown project nope"));
    }

    #[test]
    fn root_lockfile_widens_to_every_project_of_its_language() {
        let t = monorepo();
        let p =
            plan_with(t.path(), &["bun.lock"], Mode::Checkpoint, &DevelopmentSection::default());
        assert_eq!(p.scope, "workspace_wide_path_based");
        assert_eq!(p.path_classes.workspace_inputs, vec!["bun.lock"]);
        assert_eq!(p.projects, vec!["editor", "image", "tests"]);
        assert!(commands(&p).iter().all(|c| c.contains(":js-")), "{:?}", commands(&p));
    }

    #[test]
    fn downstream_expansion_follows_depends_on() {
        let t = monorepo();
        let files = &["engine/core/crates/geom/src/lib.rs"];
        let mut policy = DevelopmentSection::default();
        let p = plan_with(t.path(), files, Mode::Checkpoint, &policy);
        assert_eq!((p.downstream, p.downstream_projects.len()), ("none", 0));
        policy.downstream = Downstream::Direct;
        let p = plan_with(t.path(), files, Mode::Checkpoint, &policy);
        assert_eq!(p.downstream_projects, vec!["editor", "image"]);
        policy.downstream = Downstream::Deep;
        let p = plan_with(t.path(), files, Mode::Checkpoint, &policy);
        assert_eq!(p.downstream, "deep_moon_depends_on");
        assert_eq!(p.projects, vec!["core"]);
        assert_eq!(p.downstream_projects, vec!["editor", "image", "tests"]);
        assert!(commands(&p).contains(&"(tests) moon run tests:js-test".to_owned()));
        assert!(commands(&p).contains(&"(modules/image) moon run image:rust-check".to_owned()));
        // A dependent with a JS-only change of its own still gets its Rust half checked.
        let p = plan_with(
            t.path(),
            &[files[0], "modules/image/glue/src/a.ts"],
            Mode::Checkpoint,
            &policy,
        );
        assert_eq!(p.projects, vec!["core", "image"]);
        assert_eq!(p.downstream_projects, vec!["editor", "tests"]);
        assert!(commands(&p).contains(&"(modules/image) moon run image:rust-check".to_owned()));
        // A dependent first listed for a root lockfile still gets its Rust checks.
        let p = plan_with(t.path(), &["bun.lock", files[0]], Mode::Checkpoint, &policy);
        assert_eq!(p.scope, "workspace_wide_path_based");
        assert_eq!(p.downstream_projects, Vec::<String>::new());
        assert!(commands(&p).contains(&"(modules/image) moon run image:rust-test".to_owned()));
        assert!(commands(&p).contains(&"(modules/image) moon run image:js-test".to_owned()));
    }

    #[test]
    fn edit_checks_name_changed_crates_in_their_cargo_workspace() {
        let t = monorepo();
        let mut policy =
            DevelopmentSection { rust_edit_backend: RustEditBackend::Blaze, ..Default::default() };
        let p = plan_with(
            t.path(),
            &[
                "engine/core/crates/geom/src/lib.rs",
                "engine/core/crates/wire/tests/roundtrip.rs",
                "modules/image/image-psd/src/lib.rs",
            ],
            Mode::Edit,
            &policy,
        );
        assert_eq!(
            commands(&p),
            vec![
                "(engine/core) blaze check -p paged-geom -p paged-wire --all-targets",
                "(modules/image) blaze check -p image-psd",
            ]
        );
        // A workspace manifest or lockfile reaches the whole workspace.
        policy.rust_edit_backend = RustEditBackend::Cargo;
        let p = plan_with(t.path(), &["engine/core/Cargo.lock"], Mode::Edit, &policy);
        assert_eq!(commands(&p), vec!["(engine/core) cargo check --workspace"]);
        // Advisory edit diagnostics stay available when the scope is unknown.
        let p = plan_with(
            t.path(),
            &[".github/workflows/ci.yml", "modules/image/image-psd/src/lib.rs"],
            Mode::Edit,
            &policy,
        );
        assert_eq!(p.scope, "unknown");
        assert_eq!(commands(&p), vec!["(modules/image) cargo check -p image-psd"]);
    }

    #[test]
    fn without_moon_each_cargo_workspace_is_a_project() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        write(r, "a/Cargo.toml", "[workspace]\nmembers = [\"x\"]\n");
        write(r, "a/x/Cargo.toml", "[package]\nname = \"x\"\n");
        write(r, "b/Cargo.toml", "[package]\nname = \"b\"\n");
        let p = plan(r, &["a/x/src/lib.rs", "b/src/main.rs"], Mode::Checkpoint, FastBackend::None);
        assert_eq!(p.projects, vec!["a", "b"]);
        assert_eq!(
            commands(&p),
            vec![
                "(a) cargo check --workspace --all-targets",
                "(a) cargo test --workspace",
                "(b) cargo check --workspace --all-targets",
                "(b) cargo test --workspace",
            ]
        );
        let p = plan(r, &["a/x/src/lib.rs"], Mode::Edit, FastBackend::Cargo);
        assert_eq!(commands(&p), vec!["(a) cargo check -p x"]);
    }
}
