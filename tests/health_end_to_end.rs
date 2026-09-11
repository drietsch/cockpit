mod support;

use cockpit::model::{HealthState, RollupStatus};
use support::{ProjectBuilder, case};

/// One feature with a unit test suite; the JUnit file is produced "by CI" at controlled times.
const CONFIG: &str = "[health]\nrequired_evidence = { critical = [\"unit\"], high = [\"unit\"], normal = [], low = [] }\n";

fn base() -> ProjectBuilder {
    ProjectBuilder::new()
        .config(CONFIG)
        .file("Cargo.toml", "[package]\nname = \"app\"\nversion = \"0.1.0\"\n")
        .file("src/search/index.rs", "fn a() {}\n")
        .file("src/search/mod.rs", "mod index;\n")
        .feature(
            "FEAT-001",
            "Search",
            &["src/search/**"],
            "importance: critical\n",
            "Search things.",
        )
        .commit("Add search", "2026-03-01T09:00:00Z")
}

fn rollup(c: &cockpit::cockpit::Cockpit, cat: &str) -> RollupStatus {
    let st = c.state.load();
    st.feature("FEAT-001")
        .unwrap()
        .health
        .rollups
        .iter()
        .find(|r| r.category.as_str() == cat)
        .unwrap()
        .status
}

#[test]
fn fresh_passing_evidence_is_healthy_and_a_later_change_makes_it_stale() {
    let p = base().junit(
        "target/nextest/ci/junit.xml",
        "app-nextest",
        &[
            case("app", "search::index::tests::finds_things", "passed"),
            case("app", "search::tests::respects_permissions", "passed"),
        ],
        "2026-03-01T10:00:00Z",
    );
    let mut c = p.scan();
    {
        let st = c.state.load();
        let f = st.feature("FEAT-001").unwrap();
        assert_eq!(f.tests.len(), 2, "tests linked by path ownership: {:?}", st.unresolved);
        assert_eq!(f.health.state, HealthState::Healthy, "{:?}", f.health.reasons);
        assert_eq!(st.counts.tests_passed, 2);
        assert!(
            f.health
                .chain
                .iter()
                .any(|s| s.text.contains("after the latest implementation change"))
        );
    }
    assert_eq!(rollup(&c, "unit"), RollupStatus::Pass);

    // The implementation changes after the evidence was produced → stale, needs attention.
    p.put_commit(
        "Rework index",
        "2026-03-02T09:00:00Z",
        &[("src/search/index.rs", "fn a() { 2 }\n")],
    );
    c.scan(None).unwrap();
    let st = c.state.load();
    let f = st.feature("FEAT-001").unwrap();
    assert_eq!(f.health.state, HealthState::Attention, "{:?}", f.health.reasons);
    assert_eq!(rollup(&c, "unit"), RollupStatus::Stale);
    assert!(f.health.reasons.iter().any(|r| r.reason.as_str() == "stale_evidence"));
    assert!(st.attention.iter().any(|a| a.reason.as_str() == "stale_evidence"
        && a.feature.as_ref().unwrap().as_str() == "FEAT-001"));

    // A new run after the change → healthy again (history keeps both runs).
    p.put_junit(
        "target/nextest/ci/junit.xml",
        "app-nextest",
        &[
            case("app", "search::index::tests::finds_things", "passed"),
            case("app", "search::tests::respects_permissions", "passed"),
        ],
        "2026-03-02T10:00:00Z",
    );
    c.scan(None).unwrap();
    let st = c.state.load();
    assert_eq!(st.feature("FEAT-001").unwrap().health.state, HealthState::Healthy);
    assert_eq!(c.store.count("runs").unwrap(), 2, "each distinct report is a run in history");
    assert_eq!(st.tests.values().next().unwrap().history.len(), 2);
    let kinds: Vec<&str> = st.events.iter().map(|e| e.kind.as_str()).collect();
    assert!(kinds.contains(&"feature_changed"), "{kinds:?}");
    assert!(kinds.contains(&"feature_attention"), "{kinds:?}");
    assert!(kinds.contains(&"feature_recovered"), "{kinds:?}");
    assert!(
        kinds.contains(&"attention_opened") && kinds.contains(&"attention_resolved"),
        "{kinds:?}"
    );
    // Re-deriving is idempotent: a scan without changes adds no events.
    let n = st.events.len();
    drop(st);
    c.scan(None).unwrap();
    assert_eq!(c.state.load().events.len(), n);
}

#[test]
fn a_fresh_failure_is_broken_and_an_old_failure_is_only_attention() {
    let p = base().junit(
        "target/nextest/ci/junit.xml",
        "app-nextest",
        &[
            case("app", "search::index::tests::finds_things", "failed"),
            case("app", "search::tests::respects_permissions", "passed"),
        ],
        "2026-03-01T10:00:00Z",
    );
    let mut c = p.scan();
    let st = c.state.load();
    let f = st.feature("FEAT-001").unwrap();
    assert_eq!(f.health.state, HealthState::Broken, "{:?}", f.health.reasons);
    assert_eq!(st.counts.broken, 1);
    assert!(st.headline.contains("1 broken"));
    let top = &st.attention[0];
    assert_eq!(top.reason.as_str(), "test_failed");
    assert!(top.summary.contains("finds_things"));

    // The implementation changes after that failing run: the failure may be fixed → attention.
    p.put_commit("Fix index", "2026-03-02T09:00:00Z", &[("src/search/index.rs", "fn a() { 3 }\n")]);
    c.scan(None).unwrap();
    let st = c.state.load();
    let f = st.feature("FEAT-001").unwrap();
    assert_eq!(f.health.state, HealthState::Attention, "{:?}", f.health.reasons);
    assert_eq!(rollup(&c, "unit"), RollupStatus::StaleFail);
}

#[test]
fn flakiness_needs_history_and_silent_runs_never_count() {
    let p = base();
    let mut c = p.scan();
    // Five runs with three failures/retries in the window and a passing latest → flaky.
    for (i, status) in ["passed", "flaky", "failed", "flaky", "passed"].iter().enumerate() {
        let ts = format!("2026-03-01T1{i}:00:00Z");
        p.put_junit(
            "target/nextest/ci/junit.xml",
            "app-nextest",
            &[case("app", "search::index::tests::finds_things", status)],
            &ts,
        );
        c.scan(None).unwrap();
    }
    let st = c.state.load();
    let t = st.tests.values().next().unwrap();
    assert_eq!(t.history.len(), 5);
    assert!(t.flaky, "{:?}", t.flaky_text);
    assert_eq!(st.feature("FEAT-001").unwrap().health.state, HealthState::Attention);
    assert!(st.attention.iter().any(|a| a.reason.as_str() == "test_flaky"));

    // A run where everything is skipped is silent: it must not turn the feature green.
    let p2 = ProjectBuilder::new()
        .config(CONFIG)
        .file("Cargo.toml", "[package]\nname = \"app\"\n")
        .file("src/search/mod.rs", "")
        .feature("FEAT-001", "Search", &["src/search/**"], "importance: critical\n", "x")
        .commit("init", "2026-03-01T09:00:00Z")
        .junit(
            "target/nextest/ci/junit.xml",
            "app-nextest",
            &[
                case("app", "search::tests::a", "skipped"),
                case("app", "search::tests::b", "skipped"),
            ],
            "2026-03-01T10:00:00Z",
        );
    let c2 = p2.scan();
    let st = c2.state.load();
    assert_eq!(
        st.feature("FEAT-001").unwrap().health.state,
        HealthState::Unknown,
        "{:?}",
        st.feature("FEAT-001").unwrap().health.reasons
    );
    assert!(st.suites[0].run_count >= 1);
}

#[test]
fn tests_missing_from_the_latest_report_leave_the_matrix_but_keep_their_history() {
    let p = base()
        .config(&format!("{CONFIG}\n[[evidence]]\nid = \"app/nextest\"\nformat = \"junit\"\nkind = \"unit\"\npath = \"target/nextest/ci/junit.xml\"\n"))
        .junit(
        "target/nextest/ci/junit.xml",
        "app-nextest",
        &[
            case("app", "search::index::tests::finds_things", "passed"),
            case("app", "search::tests::old_name", "passed"),
        ],
        "2026-03-01T10:00:00Z",
    );
    let mut c = p.scan();
    assert_eq!(c.state.load().tests.len(), 2);
    // The suite is re-run; one test was renamed. The report name changed too (CI vs local
    // profile), which must not split the suite.
    p.put_junit(
        "target/nextest/ci/junit.xml",
        "app-ci",
        &[
            case("app", "search::index::tests::finds_things", "passed"),
            case("app", "search::tests::new_name", "passed"),
        ],
        "2026-03-01T11:00:00Z",
    );
    c.scan(None).unwrap();
    let st = c.state.load();
    let names: Vec<&str> = st.tests.values().map(|t| t.test.name.as_str()).collect();
    assert_eq!(st.tests.len(), 2, "{names:?}");
    assert!(
        names.iter().any(|n| n.ends_with("new_name"))
            && !names.iter().any(|n| n.ends_with("old_name")),
        "{names:?}"
    );
    assert_eq!(
        st.suites.len(),
        1,
        "a configured source is one suite: {:?}",
        st.suites.iter().map(|s| s.id.to_string()).collect::<Vec<_>>()
    );
    assert_eq!(st.suites[0].id.as_str(), "app/nextest");
    assert_eq!(c.store.count("tests").unwrap(), 3, "history keeps the renamed test");
    assert_eq!(c.store.count("runs").unwrap(), 2);
}
