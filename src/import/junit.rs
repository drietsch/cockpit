//! JUnit XML as emitted by cargo-nextest, vitest, and most other runners.

use super::batch::{BatchSpec, RawResult, assemble};
use super::provenance::Declared;
use super::{ImportContext, ImportInput, ImportOutput, ImportedEntity, Importer, ImporterKind};
use crate::config::EvidenceFormat;
use crate::model::*;
use crate::paths::RelPath;
use crate::time::{Origin, parse_timestamp};
use jiff::Timestamp;
use roxmltree::Node;

pub struct JunitImporter;

const NAME: &str = "junit";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flavour {
    Nextest,
    Vitest,
    Generic,
}

impl Importer for JunitImporter {
    fn kind(&self) -> ImporterKind {
        ImporterKind::Evidence(EvidenceFormat::Junit)
    }
    fn version(&self) -> u32 {
        3
    }
    fn import(&self, ctx: &ImportContext, input: &ImportInput) -> ImportOutput {
        let mut out = ImportOutput::default();
        let mut text = input.text();
        // Some CI steps concatenate several reports into one file (`junit-all.xml`): strip
        // the repeated declarations and wrap everything in one root.
        if text.matches("<?xml").count() > 1 {
            let mut joined = String::with_capacity(text.len() + 32);
            for chunk in text.split("<?xml") {
                if chunk.trim().is_empty() {
                    continue;
                }
                let body = match chunk.find("?>") {
                    Some(i) => &chunk[i + 2..],
                    None => chunk,
                };
                joined.push_str(body);
            }
            text = format!("<testsuites name=\"combined\">{joined}</testsuites>").into();
        }
        let doc = match roxmltree::Document::parse(&text) {
            Ok(d) => d,
            Err(e) => {
                out.fatal(
                    &input.rel,
                    NAME,
                    format!("not well-formed XML: {e}"),
                    Some(format!("{}", e.pos())),
                );
                return out;
            }
        };
        let root = doc.root_element();
        let suites: Vec<Node> = if root.has_tag_name("testsuites") {
            root.descendants().filter(|n| n.has_tag_name("testsuite")).collect()
        } else if root.has_tag_name("testsuite") {
            vec![root]
        } else {
            out.fatal(
                &input.rel,
                NAME,
                format!(
                    "root element is <{}>, expected <testsuites> or <testsuite>",
                    root.tag_name().name()
                ),
                None,
            );
            return out;
        };
        let report_name = root.attribute("name").unwrap_or("").to_string();
        let flavour = detect_flavour(&report_name, &suites);
        let mut results: Vec<RawResult> = Vec::new();
        let mut latest: Option<Timestamp> = None;
        let mut earliest_suite_ts: Option<Timestamp> = None;
        for suite in &suites {
            let suite_ts = suite.attribute("timestamp").and_then(parse_timestamp);
            if let Some(t) = suite_ts {
                if earliest_suite_ts.map(|e| t < e).unwrap_or(true) {
                    earliest_suite_ts = Some(t);
                }
            }
            let suite_name = suite.attribute("name").unwrap_or("");
            for case in suite.children().filter(|n| n.has_tag_name("testcase")) {
                let name = case.attribute("name").unwrap_or("unknown").to_string();
                let classname = case.attribute("classname").map(|s| s.to_string()).or_else(|| {
                    if suite_name.is_empty() { None } else { Some(suite_name.to_string()) }
                });
                let secs: f64 = case.attribute("time").and_then(|t| t.parse().ok()).unwrap_or(0.0);
                let ts = case.attribute("timestamp").and_then(parse_timestamp).or(suite_ts);
                if let Some(t) = ts {
                    let end = t
                        .checked_add(jiff::SignedDuration::from_millis((secs * 1000.0) as i64))
                        .unwrap_or(t);
                    if latest.map(|l| end > l).unwrap_or(true) {
                        latest = Some(end);
                    }
                }
                let mut status = TestStatus::Passed;
                let mut retries = 0u32;
                let mut message: Option<String> = None;
                let mut details: Option<String> = None;
                for child in case.children().filter(|n| n.is_element()) {
                    match child.tag_name().name() {
                        "failure" | "error" => {
                            status = TestStatus::Failed;
                            message = child.attribute("message").map(|s| s.to_string()).or(message);
                            let body = child.text().unwrap_or("").trim().to_string();
                            if !body.is_empty() {
                                details = Some(body);
                            }
                        }
                        "skipped" => {
                            if status == TestStatus::Passed {
                                status = TestStatus::Skipped;
                            }
                            message = child.attribute("message").map(|s| s.to_string()).or(message);
                        }
                        "flakyFailure" | "flakyError" => {
                            retries += 1;
                            if status == TestStatus::Passed {
                                status = TestStatus::Flaky;
                            }
                        }
                        "rerunFailure" | "rerunError" => {
                            retries += 1;
                            status = TestStatus::Failed;
                        }
                        "system-err" | "system-out" if status == TestStatus::Failed => {
                            let body = child.text().unwrap_or("").trim().to_string();
                            if !body.is_empty() && details.is_none() {
                                details = Some(body);
                            }
                        }
                        _ => {}
                    }
                }
                let mut tags: Vec<String> = Vec::new();
                let mut declared: Vec<String> = Vec::new();
                if let Some(props) = case.children().find(|n| n.has_tag_name("properties")) {
                    for p in props.children().filter(|n| n.has_tag_name("property")) {
                        if let (Some(k), Some(v)) = (p.attribute("name"), p.attribute("value")) {
                            tags.push(format!("{k}={v}"));
                            if k == "feature" || k == "features" || k == "feat" {
                                declared.extend(
                                    v.split(',')
                                        .map(|s| s.trim().to_string())
                                        .filter(|s| !s.is_empty()),
                                );
                            }
                        }
                    }
                }
                let (source_path, source_origin, kind) = match (&classname, flavour) {
                    (Some(c), Flavour::Nextest) => {
                        match ctx.cargo.resolve_test(c, &name, Some(&ctx.located.root)) {
                            Some((p, k)) => (Some(p), Some(Origin::Inferred), Some(k)),
                            None => (None, None, None),
                        }
                    }
                    (Some(c), Flavour::Vitest) => {
                        let base = package_dir_of(&input.rel, &ctx.located.root);
                        (Some(base.join(c)), Some(Origin::Inferred), None)
                    }
                    _ => (None, None, None),
                };
                let key = match &classname {
                    Some(c) => format!("{c}::{name}"),
                    None => name.clone(),
                };
                results.push(RawResult {
                    key,
                    name,
                    classname,
                    status: Some(status),
                    duration_ms: Some((secs * 1000.0).round() as u64),
                    retries,
                    timestamp: ts,
                    message,
                    details,
                    tags,
                    declared_feature_ids: declared,
                    source_path,
                    source_path_origin: source_origin,
                    kind,
                    ..Default::default()
                });
            }
        }
        let declared_ts = root.attribute("timestamp").and_then(parse_timestamp);
        let finished = latest.or(declared_ts).or(earliest_suite_ts);
        let props = root.children().find(|n| n.has_tag_name("properties"));
        let mut decl = Declared { timestamp: finished, ..Default::default() };
        if let Some(props) = props {
            for p in props.children().filter(|n| n.has_tag_name("property")) {
                match (p.attribute("name"), p.attribute("value")) {
                    (Some("commit"), Some(v))
                    | (Some("git.commit"), Some(v))
                    | (Some("sha"), Some(v)) => decl.commit = Some(v.to_string()),
                    (Some("branch"), Some(v)) | (Some("git.branch"), Some(v)) => {
                        decl.branch = Some(v.to_string())
                    }
                    _ => {}
                }
            }
        }
        decl.tool = Some(ToolInfo {
            name: match flavour {
                Flavour::Nextest => "cargo-nextest".into(),
                Flavour::Vitest => "vitest".into(),
                Flavour::Generic => "junit".into(),
            },
            version: None,
        });
        let suite_name = ctx.source.and_then(|s| s.suite.clone()).unwrap_or_else(|| {
            if report_name.is_empty() { "junit".into() } else { report_name.clone() }
        });
        let default_kind =
            ctx.source.map(|s| TestKind::parse(s.default_kind())).unwrap_or(TestKind::Unit);
        let (batch, notes) = assemble(
            ctx,
            BatchSpec {
                format: EvidenceFormat::Junit,
                version: self.version(),
                suite_name,
                default_kind,
                declared: decl,
                run_identity: None,
                results,
                input,
                source_override: None,
            },
        );
        for n in notes {
            out.warn(&input.rel, NAME, n, None);
        }
        out.entities.push(ImportedEntity::TestBatch(batch));
        out
    }
}

fn detect_flavour(report_name: &str, suites: &[Node]) -> Flavour {
    if report_name == "vitest tests" {
        return Flavour::Vitest;
    }
    let cases: Vec<Node> =
        suites.iter().flat_map(|s| s.children().filter(|n| n.has_tag_name("testcase"))).collect();
    let first_class = cases.iter().find_map(|c| c.attribute("classname").map(|s| s.to_string()));
    if let Some(c) = &first_class {
        if c.ends_with(".ts")
            || c.ends_with(".tsx")
            || c.ends_with(".js")
            || c.ends_with(".mjs")
            || c.ends_with(".jsx")
            || c.ends_with(".py")
        {
            return Flavour::Vitest;
        }
    }
    // nextest: `package::binary` classnames, or module paths (`a::b::test`) in test names, or
    // its `uuid` attribute on <testsuites>.
    let rusty = first_class.as_deref().map(|c| c.contains("::")).unwrap_or(false)
        || suites.iter().any(|s| s.attribute("name").map(|n| n.contains("::")).unwrap_or(false))
        || cases.iter().any(|c| c.attribute("name").map(|n| n.contains("::")).unwrap_or(false))
        || suites.first().and_then(|s| s.parent()).and_then(|p| p.attribute("uuid")).is_some();
    if rusty { Flavour::Nextest } else { Flavour::Generic }
}

/// The nearest ancestor directory of the report that holds a `package.json` (vitest paths
/// are relative to it); falls back to the report's own directory.
fn package_dir_of(rel: &RelPath, root: &std::path::Path) -> RelPath {
    let mut dir = rel.parent();
    loop {
        if dir.join("package.json").to_abs(root).exists() {
            return dir;
        }
        if dir.is_root() {
            return rel.parent();
        }
        dir = dir.parent();
    }
}

#[cfg(test)]
mod tests {
    use super::super::feature_md::tests::{input, located, project};
    use super::*;
    use crate::import::cargo_index::CargoWorkspaceIndex;
    use std::collections::HashSet;

    fn run(
        rel: &str,
        xml: &str,
        cargo: &CargoWorkspaceIndex,
    ) -> (super::super::TestBatch, Vec<ImportWarning>) {
        let loc = located();
        let proj = project();
        let repos = super::super::RepoIndex::default();
        let ids: HashSet<crate::ids::FeatureId> = HashSet::new();
        let ctx = ImportContext {
            located: &loc,
            project: &proj,
            repos: &repos,
            feature_ids: &ids,
            source: None,
            source_id: None,
            now: crate::time::parse_timestamp("2026-09-11T12:00:00Z").unwrap(),
            cargo,
            sidecars: &Default::default(),
            commits: &Default::default(),
        };
        let out = JunitImporter.import(&ctx, &input(rel, xml));
        match out.entities.into_iter().next() {
            Some(ImportedEntity::TestBatch(b)) => (b, out.warnings),
            _ => panic!("no batch: {:?}", out.warnings),
        }
    }

    const NEXTEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="paged-core-nextest" tests="4" failures="1" errors="0" uuid="d5a931e1" timestamp="2026-09-09T14:14:51.502+02:00" time="0.244">
    <testsuite name="paged-canvas::frame_mutation_bridge" tests="3" disabled="0" errors="0" failures="1">
        <testcase name="mixed_batch_rolls_back__feat__layers_ops" classname="paged-canvas::frame_mutation_bridge" timestamp="2026-09-09T14:14:51.504+02:00" time="0.123"></testcase>
        <testcase name="resize_frame_routes [FEAT-019]" classname="paged-canvas::frame_mutation_bridge" timestamp="2026-09-09T14:14:51.511+02:00" time="0.118"><failure message="assertion failed">left: 1
right: 2</failure></testcase>
        <testcase name="flaky_one" classname="paged-canvas::frame_mutation_bridge" timestamp="2026-09-09T14:14:51.520+02:00" time="0.5"><flakyFailure message="first attempt">boom</flakyFailure></testcase>
    </testsuite>
    <testsuite name="paged-canvas" tests="1">
        <testcase name="unit_in_lib" classname="paged-canvas" timestamp="2026-09-09T14:14:52.000+02:00" time="0.001"><skipped message="ignored: scale"/></testcase>
    </testsuite>
</testsuites>"#;

    #[test]
    fn nextest_report() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("core/crates/paged-canvas/tests")).unwrap();
        std::fs::write(
            tmp.path().join("core/crates/paged-canvas/tests/frame_mutation_bridge.rs"),
            "",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("core/crates/paged-canvas/Cargo.toml"),
            "[package]\nname = \"paged-canvas\"\n",
        )
        .unwrap();
        let cargo = CargoWorkspaceIndex::build(
            tmp.path(),
            &[RelPath::new("core/crates/paged-canvas/Cargo.toml")],
        );
        let (b, warnings) = run("core/target/nextest/ci/junit.xml", NEXTEST, &cargo);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(b.tests.len(), 4);
        assert_eq!(b.run.counts, RunCounts { passed: 1, failed: 1, skipped: 1, flaky: 1 });
        assert!(!b.run.silent);
        let t0 = &b.tests[0];
        assert_eq!(
            t0.key,
            "paged-canvas::frame_mutation_bridge::mixed_batch_rolls_back__feat__layers_ops"
        );
        assert_eq!(t0.declared_feature_ids, vec!["flat:layers_ops"]);
        assert_eq!(t0.kind, TestKind::Integration);
        assert_eq!(b.tests[1].declared_feature_ids, vec!["FEAT-019"]);
        assert_eq!(b.test_runs[1].status, TestStatus::Failed);
        assert_eq!(b.test_runs[1].message.as_deref(), Some("assertion failed"));
        assert!(b.test_runs[1].details.as_deref().unwrap().contains("right: 2"));
        assert_eq!(b.test_runs[2].status, TestStatus::Flaky);
        assert_eq!(b.test_runs[2].retries, 1);
        assert_eq!(b.tests[3].kind, TestKind::Unit);
        assert_eq!(b.test_runs[3].status, TestStatus::Skipped);
        // finished_at = latest case end: the flaky case at 14:14:51.520 + 0.5 s (+02:00 → Z).
        assert_eq!(b.run.finished_at.value.to_string(), "2026-09-09T12:14:52.02Z");
        assert_eq!(b.run.finished_at.origin, Origin::Declared);
        assert_eq!(b.run.provenance.tool.as_ref().unwrap().name, "cargo-nextest");
        // Source path resolution via the cargo index: the integration binary maps under tests/
        // (the fixture root is not on disk, so the directory form is used).
        assert_eq!(t0.source_path.as_ref().unwrap().as_str(), "core/crates/paged-canvas/tests");
    }

    const VITEST: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<testsuites name="vitest tests" tests="2" failures="0" errors="0" time="3.9">
    <testsuite name="test/account.test.ts" timestamp="2026-06-20T09:54:49.615Z" hostname="h" tests="2" failures="0" errors="0" skipped="0" time="0.8">
        <testcase classname="test/account.test.ts" name="account &gt; creates the profile" time="0.006"></testcase>
        <testcase classname="test/account.test.ts" name="account &gt; changes the password" time="0.244"><system-err>noise</system-err></testcase>
    </testsuite>
</testsuites>"#;

    #[test]
    fn vitest_report_and_malformed() {
        let (b, _) = run("editor-server/test-results/junit.xml", VITEST, &Default::default());
        assert_eq!(b.tests.len(), 2);
        assert_eq!(b.tests[0].name, "account > creates the profile");
        assert_eq!(
            b.tests[0].source_path.as_ref().unwrap().as_str(),
            "editor-server/test-results/test/account.test.ts"
        );
        assert_eq!(b.run.provenance.tool.as_ref().unwrap().name, "vitest");
        assert_eq!(b.run.finished_at.value.to_string(), "2026-06-20T09:54:49.859Z");

        let loc = located();
        let proj = project();
        let repos = super::super::RepoIndex::default();
        let ids: HashSet<crate::ids::FeatureId> = HashSet::new();
        let ctx = ImportContext {
            located: &loc,
            project: &proj,
            repos: &repos,
            feature_ids: &ids,
            source: None,
            source_id: None,
            now: crate::time::now(),
            cargo: &Default::default(),
            sidecars: &Default::default(),
            commits: &Default::default(),
        };
        let out = JunitImporter.import(&ctx, &input("x/junit.xml", "<testsuites><testsuite"));
        assert!(out.entities.is_empty());
        assert!(out.warnings[0].fatal);
    }
}
