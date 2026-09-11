mod support;

use cockpit::model::HealthState;
use support::ProjectBuilder;

fn project() -> ProjectBuilder {
    ProjectBuilder::new()
        .feature(
            "FEAT-001",
            "Search",
            &["src/search/**"],
            "importance: critical\nadrs:\n  - ADR-0001\n",
            "Search things.\n\n## Acceptance criteria\n\n- Fast\n- Correct\n",
        )
        .feature(
            "FEAT-002",
            "Billing",
            &["src/billing/**"],
            "importance: low\nadrs:\n  - ADR-0099\n",
            "Bill things.",
        )
        .adr(1, "Use an inverted index", "Accepted", "2026-02-01", "Because [FEAT-001] needs it.")
        .adr(2, "Old idea", "Superseded by ADR-0001", "2026-01-15", "Nope.")
        .file("src/search/index.rs", "fn a() {}\n")
        .file("src/billing/invoice.rs", "fn b() {}\n")
        .commit("Initial layout [FEAT-001]", "2026-03-01T09:00:00Z")
        .commit_touching(
            "Replace search index",
            "2026-03-02T09:00:00Z",
            &[("src/search/index.rs", "fn a() { /* v2 */ }\n")],
        )
        .commit_touching(
            "Tweak invoice rounding",
            "2026-03-03T09:00:00Z",
            &[("src/billing/invoice.rs", "fn b() { 1 }\n")],
        )
        .tag("v1.0.0")
}

#[test]
fn scan_links_commits_adrs_and_reports_unknown_health() {
    let p = project();
    let c = p.scan();
    let st = c.state.load();
    assert_eq!(st.counts.features, 2);
    assert_eq!(st.counts.adrs, 2);
    assert_eq!(st.counts.commits, 3);
    let search = st.feature("FEAT-001").unwrap();
    // No evidence yet → Unknown, never Healthy.
    assert_eq!(search.health.state, HealthState::Unknown);
    assert!(
        search.health.reasons.iter().any(|r| r.reason.as_str() == "missing_required_test"),
        "{:?}",
        search.health.reasons
    );
    // Latest implementation commit is the search-index change, not the later billing one.
    let impl_commit = st.commits.get(search.impl_commit.as_ref().unwrap()).unwrap();
    assert_eq!(impl_commit.commit.subject, "Replace search index");
    // Explicit vs path-based correlation.
    let first = st.commits.values().find(|c| c.commit.subject.starts_with("Initial")).unwrap();
    assert!(first.features.iter().any(|(id, o)| id.as_str() == "FEAT-001" && o.is_explicit()));
    let billing = st.commits.values().find(|c| c.commit.subject.starts_with("Tweak")).unwrap();
    assert!(billing.features.iter().any(|(id, o)| id.as_str() == "FEAT-002" && !o.is_explicit()));
    // ADR links: explicit from frontmatter, mention in ADR body, superseded chain, dangling ref.
    let adr1 = st.adrs.get(&"ADR-0001".into()).unwrap();
    assert!(adr1.features.iter().any(|f| f.as_str() == "FEAT-001"));
    assert_eq!(
        st.adrs.get(&"ADR-0002".into()).unwrap().adr.superseded_by.as_ref().unwrap().as_str(),
        "ADR-0001"
    );
    assert!(st.unresolved.iter().any(|u| u.raw == "ADR-0099"), "{:?}", st.unresolved);
    assert!(st.attention.iter().any(|a| a.reason.as_str() == "unresolved_reference"));
    assert!(st.tags.iter().any(|(_, t, _)| t == "v1.0.0"));
    // Acceptance criteria parsed.
    assert_eq!(search.feature.acceptance_criteria.len(), 2);
}

#[test]
fn rescan_is_idempotent_and_incremental() {
    let p = project();
    let mut c = p.scan();
    let gen1 = c.state.load().generation;
    let first_seen = c.state.load().attention[0].first_seen;
    let r2 = c.scan(None).unwrap();
    assert_eq!(r2.counts.files_imported, 0, "unchanged files are not re-read");
    assert_eq!(r2.counts.new_commits, 0);
    let st = c.state.load();
    assert_eq!(st.generation, gen1 + 1);
    assert_eq!(st.attention[0].first_seen, first_seen, "attention first_seen survives rescans");
    assert_eq!(c.store.count("commits").unwrap(), 3);
}

#[test]
fn full_scan_equals_incremental_after_changes() {
    let p = project();
    let mut c = p.scan();
    // Add a feature and a commit, then rescan incrementally.
    std::fs::write(
        p.root.join("docs/features/FEAT-003.md"),
        "---\nid: FEAT-003\ntitle: Export\npaths:\n  - src/export/**\n---\n# Export\n",
    )
    .unwrap();
    std::fs::create_dir_all(p.root.join("src/export")).unwrap();
    std::fs::write(p.root.join("src/export/pdf.rs"), "fn c() {}\n").unwrap();
    support::git(&p.root, &["add", "-A"], "2026-03-04T09:00:00Z");
    support::git(&p.root, &["commit", "-q", "-m", "Add export"], "2026-03-04T09:00:00Z");
    c.scan(None).unwrap();
    let incremental = c.state.load();
    // A fresh index from scratch must agree.
    let tmp = tempfile::tempdir().unwrap();
    let mut fresh =
        cockpit::cockpit::Cockpit::from_located(p.located(), Some(&tmp.path().join("idx.sqlite")))
            .unwrap();
    fresh.scan(None).unwrap();
    let full = fresh.state.load();
    assert_eq!(incremental.counts.features, full.counts.features);
    assert_eq!(incremental.counts.commits, full.counts.commits);
    assert_eq!(
        incremental.feature("FEAT-003").unwrap().impl_commit,
        full.feature("FEAT-003").unwrap().impl_commit
    );
    assert_eq!(incremental.unresolved.len(), full.unresolved.len());
}
