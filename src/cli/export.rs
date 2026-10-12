//! `cockpit export [--profile public] [--out FILE]`
//!
//! One JSON document a publishing pipeline can build from: every feature with its lifecycle,
//! its claims per component, and the health its linked evidence gives it, plus the components
//! and chapters to group them by. The `public` profile leaves out what only the team needs
//! (commit subjects, attention items, benchmarks, file paths); `full` keeps the feature notes
//! and evidence references as well.

use super::output::Format;
use super::{EXIT_OK, EXIT_USAGE};
use crate::cockpit::Cockpit;
use std::collections::BTreeMap;
use std::path::Path;

pub const SCHEMA: &str = "cockpit.export.v1";

pub fn run(c: &Cockpit, profile: &str, out: Option<&Path>, fmt: &Format) -> anyhow::Result<i32> {
    if profile != "public" && profile != "full" {
        fmt.error(EXIT_USAGE, "export", &format!("unknown profile {profile:?} (public, full)"));
        return Ok(EXIT_USAGE);
    }
    let full = profile == "full";
    let st = c.state.load();
    let root = &c.located.root;

    let mut chapters: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    let mut by_status: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_health: BTreeMap<&str, usize> = BTreeMap::new();
    let mut features = Vec::with_capacity(st.features.len());

    for v in st.features.values() {
        let f = &v.feature;
        let chapter = chapter_of(f.source.as_str());
        chapters
            .entry(chapter.clone())
            .or_insert_with(|| chapter_json(root, f.source.as_str(), &chapter));
        *by_status.entry(f.lifecycle.as_str()).or_default() += 1;
        *by_health.entry(v.health.state.as_str()).or_default() += 1;

        let mut passed = 0;
        let mut failed = 0;
        let mut skipped = 0;
        let mut evidence_at = None;
        for r in &v.health.rollups {
            passed += r.counts.passed;
            failed += r.counts.failed;
            skipped += r.counts.skipped;
            if r.latest > evidence_at {
                evidence_at = r.latest;
            }
        }
        let claims: Vec<serde_json::Value> = f
            .claims
            .iter()
            .map(|cl| {
                let mut o =
                    serde_json::json!({ "component": cl.component, "status": cl.status.as_str() });
                if let Some(n) = &cl.note {
                    o["note"] = serde_json::Value::from(n.as_str());
                }
                o
            })
            .collect();
        let mut fj = serde_json::json!({
            "id": f.id,
            "title": f.title,
            "chapter": chapter,
            "status": f.lifecycle.as_str(),
            "importance": f.importance.as_str(),
            "since": f.since,
            "claims": claims,
            "health": {
                "state": v.health.state.as_str(),
                "headline": v.health.headline,
                "evidence_at": evidence_at.map(|t| t.to_string()),
                "tests": { "linked": v.tests.len(), "passed": passed, "failed": failed, "skipped": skipped },
            },
        });
        if full {
            fj["description"] = serde_json::json!(f.description);
            fj["references"] = serde_json::json!(f.references);
            fj["source"] = serde_json::json!(f.source.as_str());
        }
        features.push(fj);
    }

    let doc = serde_json::json!({
        "schema": SCHEMA,
        "profile": profile,
        "generated_at": crate::time::now().to_string(),
        "last_scan_at": st.last_scan_at.map(|t| t.to_string()),
        "project": { "id": st.project.id, "name": st.project.name },
        "repos": st.project.repos.iter().filter_map(|r| r.head.as_ref().map(|h| serde_json::json!({ "id": r.id, "sha": h.sha }))).collect::<Vec<_>>(),
        "evidence": st.sources.iter().filter(|s| s.enabled).map(|s| serde_json::json!({
            "id": s.id, "kind": s.kind, "component": s.component, "runs": s.runs,
            "last_seen": s.last_seen.map(|t| t.to_string()),
        })).collect::<Vec<_>>(),
        "components": st.project.components.iter().map(|c| serde_json::json!({ "id": c.id, "title": c.title, "group": c.group, "order": c.order })).collect::<Vec<_>>(),
        "chapters": chapters.into_values().collect::<Vec<_>>(),
        "totals": { "features": features.len(), "by_status": by_status, "by_health": by_health },
        "features": features,
    });
    let text = serde_json::to_string_pretty(&doc)? + "\n";
    match out {
        Some(p) => {
            if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(p, text)?;
            if !fmt.json {
                eprintln!("wrote {} ({} features)", p.display(), st.features.len());
            }
        }
        None => print!("{text}"),
    }
    Ok(EXIT_OK)
}

/// The chapter is the folder a feature file sits in (`docs/features/<chapter>/<id>.md`).
fn chapter_of(source: &str) -> String {
    let p = Path::new(source);
    p.parent()
        .and_then(|d| d.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Title, group and order from the chapter's `_chapter.md` frontmatter, when there is one.
fn chapter_json(root: &Path, source: &str, id: &str) -> serde_json::Value {
    let file = root.join(source).with_file_name("_chapter.md");
    let mut o = serde_json::json!({ "id": id, "title": id });
    if let Ok(text) = std::fs::read_to_string(file) {
        for line in text.lines().skip(1).take_while(|l| l.trim() != "---") {
            if let Some((k, v)) = line.split_once(':') {
                let v = v.trim().trim_matches(['"', '\'']);
                match k.trim() {
                    "title" => o["title"] = v.into(),
                    "group" => o["group"] = v.into(),
                    "order" => {
                        o["order"] =
                            v.parse::<u32>().map(Into::into).unwrap_or(serde_json::Value::Null)
                    }
                    _ => {}
                }
            }
        }
    }
    o
}
