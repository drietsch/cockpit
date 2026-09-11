//! `cockpit init [--from-cargo]`: scaffold cockpit.toml, docs/features and docs/adr from what
//! the repository already has. Never overwrites.

use super::output::Format;
use std::path::Path;

pub fn run(root: &Path, from_cargo: bool, fmt: &Format) -> anyhow::Result<i32> {
    let mut written: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut write = |rel: &str, content: String| -> anyhow::Result<()> {
        let p = root.join(rel);
        if p.exists() {
            skipped.push(rel.to_string());
            return Ok(());
        }
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&p, content)?;
        written.push(rel.to_string());
        Ok(())
    };

    // Detect what exists.
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into());
    let repos: Vec<String> = {
        let mut v = Vec::new();
        if root.join(".git").exists() {
            v.push(".".into());
        } else if let Ok(rd) = std::fs::read_dir(root) {
            for e in rd.flatten() {
                if e.path().join(".git").exists() {
                    v.push(e.file_name().to_string_lossy().to_string());
                }
            }
        }
        v.sort();
        v
    };
    let has = |rel: &str| root.join(rel).exists();
    let cargo = has("Cargo.toml");
    let package = has("package.json");
    let nextest = cargo && has(".config/nextest.toml");
    let adr_dir = ["docs/adr", "docs/adrs", "docs/decisions", "adr"]
        .iter()
        .find(|d| has(d))
        .map(|s| s.to_string());
    let mut toml = String::new();
    toml.push_str(&format!("# Project Cockpit configuration — see docs/config.md in the Cockpit repository.\n[project]\nid = \"{}\"\nname = \"{}\"\n", slug(&name), name));
    if repos.len() > 1 {
        toml.push_str("\n# An umbrella of sibling clones: each repository listed once.\n");
        for r in &repos {
            toml.push_str(&format!("[[repos]]\nid = \"{r}\"\npath = \"{r}\"\n\n"));
        }
    }
    toml.push_str("\n[features]\npath = \"docs/features\"\n# id_pattern = \"^FEAT-\\\\d{3}$\"\n\n");
    toml.push_str(&format!(
        "[adrs]\npath = \"{}\"\nexclude = [\"0000-template.md\", \"README.md\"]\n\n",
        adr_dir.clone().unwrap_or_else(|| "docs/adr".into())
    ));
    toml.push_str(
        "[git]\nimplementation_paths_exclude = [\"docs/**\", \"**/*.md\", \"cockpit.toml\"]\n\n",
    );
    if cargo {
        toml.push_str("# cargo nextest run --workspace --profile ci  →  target/nextest/ci/junit.xml\n[[evidence]]\nid = \"nextest\"\nformat = \"junit\"\nkind = \"unit\"\npath = \"target/nextest/ci/junit.xml\"\n\n");
        toml.push_str("# [[evidence]]\n# id = \"criterion\"\n# format = \"criterion\"\n# kind = \"benchmark\"\n# path = \"target/criterion/**/new/estimates.json\"\n\n");
    }
    if package {
        toml.push_str("# [[evidence]]\n# id = \"vitest\"\n# format = \"junit\"\n# kind = \"unit\"\n# path = \"**/test-results/junit.xml\"\n\n# [[evidence]]\n# id = \"playwright\"\n# format = \"playwright\"\n# kind = \"e2e\"\n# path = \"**/test-results/results.json\"\n\n");
    }
    toml.push_str("# [[evidence]]\n# id = \"ci/nextest\"\n# format = \"junit\"\n# kind = \"unit\"\n# path = \".cockpit/evidence/ci__nextest/**\"\n# [evidence.github]\n# repo = \"owner/repo\"\n# workflow = \"ci.yml\"\n# artifact = \"nextest-junit\"\n\n");
    toml.push_str("[health]\nmax_evidence_age = \"7d\"\nrequired_evidence = { critical = [\"test\"], high = [\"test\"], normal = [], low = [] }\n");
    write("cockpit.toml", toml)?;
    if cargo && !nextest {
        write(".config/nextest.toml", "[profile.ci]\nfail-fast = false\n\n[profile.ci.junit]\npath = \"junit.xml\"\nreport-name = \"nextest\"\n".into())?;
    }
    write("docs/features/README.md", "# Features\n\nOne Markdown file per feature with YAML frontmatter. Health is computed by Cockpit from evidence (test reports, benchmarks, commits) and never written here.\n\n```markdown\n---\nid: FEAT-001\ntitle: Search\nstatus: stable          # planned | experimental | beta | stable | deprecated | retired\nimportance: high        # critical | high | normal | low\npaths:\n  - crates/search/**\nadrs:\n  - ADR-0001\n---\n\n# Search\n\nWhat the feature is and where it lives.\n\n## Acceptance criteria\n\n- Results come back within 200 ms\n```\n\nIds are stable; rename with `superseded_by`. `paths:` drive test and commit attribution.\n".into())?;
    let features_empty = std::fs::read_dir(root.join("docs/features"))
        .map(|d| {
            d.flatten()
                .filter(|e| {
                    e.path().extension().map(|x| x == "md").unwrap_or(false)
                        && e.file_name() != "README.md"
                })
                .count()
                == 0
        })
        .unwrap_or(true);
    if from_cargo && cargo {
        let located = crate::config::Config::discover(root, None).ok();
        let manifests = located.as_ref().map(crate::scan::walk::manifests).unwrap_or_default();
        let mut by_layer: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for m in &manifests {
            let dir = m.parent();
            if dir.as_str().is_empty() {
                continue;
            }
            let comps: Vec<&str> = dir.as_str().split('/').collect();
            let layer = if comps.len() >= 2 {
                comps[comps.len() - 2].to_string()
            } else {
                comps[0].to_string()
            };
            by_layer.entry(layer).or_default().push(format!("{}/**", dir.as_str()));
        }
        for (n, (layer, paths)) in (1..).zip(by_layer) {
            let id = format!("FEAT-{n:03}");
            let title = layer.replace(['-', '_'], " ");
            let title = title[..1].to_uppercase() + &title[1..];
            let mut fm = format!(
                "---\nid: {id}\ntitle: {title}\nstatus: stable\nimportance: normal\npaths:\n"
            );
            for p in &paths {
                fm.push_str(&format!("  - {p}\n"));
            }
            fm.push_str(&format!("---\n\n# {title}\n\nProposed from the Cargo workspace layout ({} crate{}). Edit the title, importance and description; split it if the crates are separate features.\n", paths.len(), if paths.len() == 1 { "" } else { "s" }));
            write(&format!("docs/features/{n:03}-{}.md", slug(&layer)), fm)?;
        }
    } else if features_empty {
        write(
            "docs/features/001-example.md",
            format!(
                "---\nid: FEAT-001\ntitle: Example feature\nstatus: experimental\nimportance: normal\npaths:\n  - {}\n---\n\n# Example feature\n\nReplace this with the first real feature. Its `paths:` decide which tests and commits belong to it.\n",
                "src/**"
            ),
        )?;
    }
    if adr_dir.is_none() {
        write("docs/adr/0000-template.md", "# ADR-0000 — Title\n\n**Status:** Proposed\n**Date:** YYYY-MM-DD\n\n## Context\n\n## Decision\n\n## Consequences\n".into())?;
    }
    if fmt.json {
        println!(
            "{}",
            serde_json::json!({ "schema": "cockpit.init.v1", "written": written, "skipped": skipped })
        );
    } else {
        for w in &written {
            println!("{} {w}", fmt.paint("32", "wrote  "));
        }
        for s in &skipped {
            println!("{} {s} (exists, left untouched)", fmt.dim("skipped"));
        }
        if written.is_empty() {
            println!("{}", fmt.dim("nothing to do — everything already exists"));
        } else {
            println!("\nNext: fill in docs/features, then `cockpit status` and `cockpit serve`.");
        }
    }
    Ok(0)
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}
