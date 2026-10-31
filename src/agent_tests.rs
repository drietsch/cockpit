//! Conservative test-impact hints and exact Playwright file selection.
//!
//! Test links originate in the previous Cockpit evidence scan. They are not
//! dynamic coverage and never establish that other tests are unaffected.
use crate::agent::Action;
use crate::link::PathOwnership;
use crate::model::{Feature, TestKind};
use crate::paths::RelPath;
use crate::state::ProjectState;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const MAX_CANDIDATES: usize = 32;
const MAX_ACTIONS: usize = 12;

#[derive(Clone, Debug, Serialize)]
pub struct TestCandidate {
    pub id: String,
    pub kind: String,
    pub path: Option<String>,
    pub prior_status: Option<String>,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct TestSelection {
    pub status: &'static str,
    pub provenance: &'static str,
    pub matched_features: Vec<String>,
    pub total_candidates: usize,
    pub omitted_candidates: usize,
    pub candidates: Vec<TestCandidate>,
    pub runnable_actions: usize,
    pub non_runnable_candidates: usize,
    pub notes: Vec<&'static str>,
}

pub struct TestPlan {
    pub selection: TestSelection,
    pub actions: Vec<Action>,
}

fn playwright_config(root: &Path, source: &str) -> Option<(String, String)> {
    let relative = Path::new(source);
    if relative.is_absolute()
        || relative.components().any(|c| !matches!(c, std::path::Component::Normal(_)))
        || !source.ends_with(".spec.ts")
            && !source.ends_with(".spec.tsx")
            && !source.ends_with(".spec.js")
            && !source.ends_with(".spec.jsx")
            && !source.ends_with(".spec.mts")
    {
        return None;
    }
    let absolute = root.join(relative);
    if !absolute.is_file() {
        return None;
    }
    let mut dir = absolute.parent()?;
    loop {
        if !dir.starts_with(root) {
            return None;
        }
        if [
            "playwright.config.ts",
            "playwright.config.mts",
            "playwright.config.js",
            "playwright.config.mjs",
            "playwright.config.cts",
            "playwright.config.cjs",
        ]
        .iter()
        .any(|name| dir.join(name).is_file())
        {
            let cwd = dir.strip_prefix(root).ok()?.to_string_lossy();
            let file = absolute.strip_prefix(dir).ok()?.to_string_lossy();
            return Some((
                if cwd.is_empty() { ".".into() } else { cwd.into_owned() },
                file.into_owned(),
            ));
        }
        if dir == root {
            break;
        }
        dir = dir.parent()?;
    }
    None
}

fn is_playwright(kind: TestKind, file: &str) -> bool {
    matches!(kind, TestKind::E2e | TestKind::Visual | TestKind::Smoke)
        && (file.ends_with(".spec.ts")
            || file.ends_with(".spec.tsx")
            || file.ends_with(".spec.js")
            || file.ends_with(".spec.jsx")
            || file.ends_with(".spec.mts"))
}

/// Build a plan from links already present in Cockpit. Does not rescan or
/// interpret prior passing tests as proof of changed code.
pub fn select(root: &Path, state: &ProjectState, changed: &[String]) -> TestPlan {
    let mut selection = TestSelection {
        status: "unavailable_not_scanned",
        provenance: "cockpit_prior_scan_feature_and_test_links",
        matched_features: vec![],
        total_candidates: 0,
        omitted_candidates: 0,
        candidates: vec![],
        runnable_actions: 0,
        non_runnable_candidates: 0,
        notes: vec![
            "Prior test links are hints, not complete code coverage.",
            "Only existing Playwright spec files with a discoverable config yield exact runnable actions.",
            "Other Rust/TypeScript tests remain part of the checkpoint/CI suites.",
            "A passed selected test does not replace mandatory project CI.",
        ],
    };
    if state.last_scan_at.is_none() {
        return TestPlan { selection, actions: vec![] };
    }

    let features: Vec<Feature> = state.features.values().map(|v| v.feature.clone()).collect();
    let ownership = PathOwnership::build(&features);
    let mut feature_ids = BTreeSet::new();
    for p in changed {
        for (id, _) in ownership.owners_of(&RelPath::new(p)) {
            feature_ids.insert(id.to_string());
        }
    }
    selection.matched_features = feature_ids.iter().take(24).cloned().collect();
    let mut candidates = BTreeMap::<String, TestCandidate>::new();
    for id in &feature_ids {
        if let Some(feature) = state.feature(id) {
            for test_id in &feature.tests {
                if let Some(t) = state.tests.get(test_id) {
                    let candidate = TestCandidate {
                        id: t.test.id.to_string(),
                        kind: t.test.kind.as_str().to_owned(),
                        path: t.test.source_path.as_ref().map(|p| p.as_str().to_owned()),
                        prior_status: t.latest.as_ref().map(|r| r.status.as_str().to_owned()),
                        reason: "linked_feature",
                    };
                    candidates.entry(candidate.id.clone()).or_insert(candidate);
                }
            }
        }
    }
    // A modified spec should be selected even if no feature claims its path.
    let changed_set: BTreeSet<&str> = changed.iter().map(String::as_str).collect();
    for t in state.tests.values() {
        if let Some(path) = &t.test.source_path {
            if changed_set.contains(path.as_str()) {
                candidates.entry(t.test.id.to_string()).or_insert_with(|| TestCandidate {
                    id: t.test.id.to_string(),
                    kind: t.test.kind.as_str().to_owned(),
                    path: Some(path.as_str().to_owned()),
                    prior_status: t.latest.as_ref().map(|r| r.status.as_str().to_owned()),
                    reason: "changed_test_file",
                });
            }
        }
    }

    selection.total_candidates = candidates.len();
    selection.omitted_candidates = candidates.len().saturating_sub(MAX_CANDIDATES);
    selection.candidates = candidates.values().take(MAX_CANDIDATES).cloned().collect();

    // Multiple Playwright test cases may map to the same spec file. Execute
    // each file once, rather than issuing a command for each test case.
    let mut commands = BTreeMap::<(String, String), Action>::new();
    for candidate in candidates.values() {
        let Some(ref path) = candidate.path else { continue };
        let kind = match candidate.kind.as_str() {
            "e2e" => TestKind::E2e,
            "visual" => TestKind::Visual,
            "smoke" => TestKind::Smoke,
            _ => continue,
        };
        if !is_playwright(kind, path) {
            continue;
        }
        if let Some((cwd, spec)) = playwright_config(root, path) {
            commands.entry((cwd.clone(), spec.clone())).or_insert(Action {
                stage: "edit",
                cwd,
                command: vec!["bunx".into(), "playwright".into(), "test".into(), spec],
                purpose: "Selected Playwright spec file from prior feature/test linkage",
                authority: "advisory_selected_test_not_complete_coverage",
                status: "recommended_not_run",
            });
        }
    }
    let count = commands.len();
    selection.runnable_actions = count;
    selection.non_runnable_candidates = candidates.len().saturating_sub(count);
    selection.status = if candidates.is_empty() {
        "no_linked_tests_not_proof_of_no_impact"
    } else if count == 0 {
        "candidates_without_exact_runner"
    } else if count > MAX_ACTIONS {
        "too_many_actions_no_execution"
    } else {
        "advisory_runnable_selection"
    };
    TestPlan {
        actions: if count > MAX_ACTIONS { vec![] } else { commands.into_values().collect() },
        selection,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Project;
    use std::fs;

    fn empty_state(root: &Path) -> ProjectState {
        ProjectState::empty(Project {
            id: "test".into(),
            name: "Test".into(),
            root: root.to_path_buf(),
            repos: vec![],
            components: vec![],
            primary_branch: "main".into(),
        })
    }

    #[test]
    fn linked_playwright_cases_collapse_to_one_file_action() {
        use crate::ids::{FeatureId, SuiteId, TestId};
        use crate::model::{Confidence, FeatureHealth, HealthState, Importance, Lifecycle, Test};
        use crate::state::{FeatureView, TestView};

        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("apps/editor/src/login.tsx");
        let spec = t.path().join("apps/editor/tests/login.spec.ts");
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        fs::create_dir_all(spec.parent().unwrap()).unwrap();
        fs::write(&src, "export const login = true;").unwrap();
        fs::write(&spec, "test('login', () => {});").unwrap();
        fs::write(t.path().join("apps/editor/playwright.config.ts"), "export default {};").unwrap();

        let mut state = empty_state(t.path());
        let now = crate::time::now();
        state.last_scan_at = Some(now);
        let feature_id = FeatureId::new("FEAT-login");
        let linked = vec![TestId::new("pw-1"), TestId::new("pw-2")];
        let feature = Feature {
            id: feature_id.clone(),
            title: "Login".into(),
            description: None,
            lifecycle: Lifecycle::Stable,
            importance: Importance::High,
            owners: vec![],
            tags: vec![],
            paths: vec!["apps/editor/src/**".into()],
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
            source: RelPath::new("docs/features/login.md"),
            extra: Default::default(),
        };
        state.features.insert(
            feature_id,
            FeatureView {
                feature,
                health: FeatureHealth {
                    state: HealthState::Unknown,
                    confidence: Confidence::Low,
                    headline: "Unknown".into(),
                    reasons: vec![],
                    rollups: vec![],
                    chain: vec![],
                    impl_commit: None,
                    impl_committed_at: None,
                    evaluated_at: now,
                    rank_score: 0,
                },
                impl_commit: None,
                impl_committed_at: None,
                commits: vec![],
                adrs: vec![],
                tests: linked.clone(),
                benchmarks: vec![],
                visuals: vec![],
            },
        );
        for id in linked {
            let test = Test {
                id: id.clone(),
                suite: SuiteId::new("pw"),
                key: format!("login:{}", id.as_str()),
                name: "login".into(),
                classname: None,
                kind: TestKind::E2e,
                source_path: Some(RelPath::new("apps/editor/tests/login.spec.ts")),
                source_path_origin: None,
                line: None,
                tags: vec![],
                component: None,
                declared_feature_ids: vec![],
                level: None,
                first_seen: now,
                last_seen: now,
            };
            state.tests.insert(
                id,
                TestView {
                    test,
                    features: vec![],
                    latest: None,
                    history: vec![],
                    flaky: false,
                    flaky_text: None,
                    duration_trend: None,
                },
            );
        }
        let result = select(t.path(), &state, &["apps/editor/src/login.tsx".into()]);
        assert_eq!(result.selection.matched_features, vec!["FEAT-login"]);
        assert_eq!(result.selection.total_candidates, 2);
        assert_eq!(result.actions.len(), 1);
        assert_eq!(result.actions[0].cwd, "apps/editor");
        assert_eq!(
            result.actions[0].command,
            vec!["bunx", "playwright", "test", "tests/login.spec.ts"]
        );
        assert_eq!(result.selection.status, "advisory_runnable_selection");
    }

    #[test]
    fn no_scan_never_invents_test_coverage() {
        let t = tempfile::tempdir().unwrap();
        let state = empty_state(t.path());
        let p = select(t.path(), &state, &["src/app.ts".into()]);
        assert_eq!(p.selection.status, "unavailable_not_scanned");
        assert!(p.actions.is_empty());
    }

    #[test]
    fn only_existing_playwright_configs_make_runnable_specs() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        let file = root.join("apps/editor/tests/login.spec.ts");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "test('login', () => {});").unwrap();
        assert!(playwright_config(root, "apps/editor/tests/login.spec.ts").is_none());
        fs::write(root.join("apps/editor/playwright.config.ts"), "export default {};").unwrap();
        assert_eq!(
            playwright_config(root, "apps/editor/tests/login.spec.ts"),
            Some(("apps/editor".into(), "tests/login.spec.ts".into()))
        );
        assert!(playwright_config(root, "../secret.spec.ts").is_none());
    }
}
