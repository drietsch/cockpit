//! `cockpit feature <id>` — the machine-readable definition of done (§142).

use super::output::Format;
use crate::cockpit::Cockpit;
use crate::model::*;
use crate::time::{ago, now};

pub fn run(c: &Cockpit, id: &str, fmt: &Format) -> anyhow::Result<i32> {
    let st = c.state.load();
    let Some(v) = st.feature(id) else {
        let q = id.to_ascii_lowercase();
        let mut sugg: Vec<String> = st
            .features
            .values()
            .filter(|f| {
                f.feature.id.as_str().to_ascii_lowercase().contains(&q)
                    || f.feature.title.to_ascii_lowercase().contains(&q)
            })
            .map(|f| f.feature.id.0.clone())
            .collect();
        sugg.truncate(5);
        fmt.error(
            super::EXIT_USAGE,
            "unknown_feature",
            &format!(
                "no feature {id}{}",
                if sugg.is_empty() {
                    String::new()
                } else {
                    format!(" — did you mean {}?", sugg.join(", "))
                }
            ),
        );
        return Ok(super::EXIT_USAGE);
    };
    let t_now = now();
    if fmt.json {
        let evidence: serde_json::Map<String, serde_json::Value> = v
            .health
            .rollups
            .iter()
            .map(|r| {
                (
                    r.category.as_str().to_string(),
                    serde_json::json!({
                        "state": r.status.as_str(),
                        "fresh": r.freshness.as_ref().map(|f| match f.change { ChangeFreshness::Fresh => "after", ChangeFreshness::Stale => "before", ChangeFreshness::Unknown => "unknown" }),
                        "passed": r.counts.passed, "failed": r.counts.failed, "flaky": r.counts.flaky, "skipped": r.counts.skipped,
                        "latest_at": r.latest.map(|t| t.to_string()),
                        "summary": r.summary,
                        "required": r.required,
                    }),
                )
            })
            .collect();
        let mut tests_by_kind = std::collections::BTreeMap::new();
        for t in v.tests.iter().filter_map(|t| st.tests.get(t)) {
            *tests_by_kind.entry(t.test.kind.as_str()).or_insert(0) += 1;
        }
        let data = serde_json::json!({
            "feature": {
                "id": v.feature.id, "title": v.feature.title, "status": v.feature.lifecycle.as_str(), "importance": v.feature.importance.as_str(),
                "tags": v.feature.tags, "owners": v.feature.owners, "paths": v.feature.paths, "adrs": v.adrs, "dependencies": v.feature.dependencies,
                "claims": v.feature.claims, "source_path": v.feature.source,
            },
            "health": {
                "state": v.health.state.as_str(), "confidence": v.health.confidence.as_str(), "headline": v.health.headline, "evaluated_at": v.health.evaluated_at.to_string(),
                "reasons": v.health.reasons.iter().map(|r| serde_json::json!({ "type": r.reason.as_str(), "kind": r.category.map(|c| c.as_str()), "severity": r.severity.as_str(), "message": r.text, "evidence": r.evidence, "commit": r.commit })).collect::<Vec<_>>(),
                "why": v.health.chain,
            },
            "evidence": evidence,
            "freshness": {
                "latest_implementation_commit": v.impl_commit.as_ref().map(|c| serde_json::json!({ "repo": c.repo, "sha": c.sha, "at": v.impl_committed_at.map(|t| t.to_string()) })),
                "by_kind": v.health.rollups.iter().map(|r| (r.category.as_str().to_string(), serde_json::Value::from(r.freshness.as_ref().map(|f| match f.change { ChangeFreshness::Fresh => "after", ChangeFreshness::Stale => "before", ChangeFreshness::Unknown => "unknown" }).unwrap_or("none")))).collect::<serde_json::Map<_, _>>(),
            },
            "attention": st.attention.iter().filter(|a| a.feature.as_ref() == Some(&v.feature.id)).map(|a| serde_json::json!({ "id": a.id, "severity": a.severity.as_str(), "reason": a.reason.as_str(), "title": a.title, "summary": a.summary, "first_seen": a.first_seen.to_string() })).collect::<Vec<_>>(),
            "recent_commits": v.commits.iter().take(10).map(|c| serde_json::json!({ "repo": c.key.repo, "sha": c.key.sha, "subject": st.commits.get(&c.key).map(|x| x.commit.subject.clone()), "at": c.committed_at.to_string(), "relation": c.origin.key(), "note": c.origin.note() })).collect::<Vec<_>>(),
            "tests": { "linked": v.tests.len(), "by_kind": tests_by_kind },
            "links": { "html": format!("/features/{}", v.feature.id), "api": format!("/api/v1/features/{}", v.feature.id) },
        });
        fmt.json_out("cockpit.feature.v1", &st.project, data);
        return Ok(0);
    }
    println!("{}    {}", fmt.bold(&v.feature.title), fmt.dim(v.feature.id.as_str()));
    println!("{}  {}", fmt.glyph(v.health.state), v.health.headline);
    println!(
        "{}",
        fmt.dim(&format!(
            "importance {} · lifecycle {} · confidence {}",
            v.feature.importance.as_str(),
            v.feature.lifecycle.as_str(),
            v.health.confidence.as_str()
        ))
    );
    println!();
    if !v.health.reasons.is_empty() {
        println!("{}", fmt.bold("Reasons"));
        for r in &v.health.reasons {
            println!(
                "  {} {}",
                fmt.paint(if r.severity == Severity::Informational { "90" } else { "33" }, "•"),
                r.text
            );
        }
        println!();
    }
    println!("{}", fmt.bold("Evidence"));
    for r in &v.health.rollups {
        let fresh = match r.freshness.as_ref().map(|f| f.change) {
            Some(ChangeFreshness::Fresh) => fmt.paint("32", "after change"),
            Some(ChangeFreshness::Stale) => fmt.paint("33", "before change"),
            Some(ChangeFreshness::Unknown) => fmt.dim("freshness unknown"),
            None => String::new(),
        };
        println!("  {:<13} {:<14} {}  {}", r.category.label(), r.status.as_str(), r.summary, fresh);
    }
    println!();
    match (&v.impl_commit, v.impl_committed_at) {
        (Some(c), Some(t)) => {
            println!("Latest implementation change: {} · {}", c.sha.short(), ago(t, t_now))
        }
        _ => println!("{}", fmt.dim("No implementation commit known")),
    }
    if !v.health.chain.is_empty() {
        println!();
        println!("{}", fmt.bold("Why"));
        for s in &v.health.chain {
            println!("  {} {}", fmt.dim(&s.connective), s.text);
        }
    }
    Ok(0)
}
