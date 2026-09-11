use super::output::{Format, thousands};
use crate::cockpit::Cockpit;
use crate::model::HealthState;
use crate::time::{ago, now};

pub fn run(c: &Cockpit, limit: usize, fmt: &Format) -> anyhow::Result<i32> {
    let st = c.state.load();
    let t_now = now();
    if fmt.json {
        fmt.json_out(
            "cockpit.status.v1",
            &st.project,
            serde_json::json!({
                "generation": st.generation,
                "last_scan_at": st.last_scan_at.map(|t| t.to_string()),
                "overall": st.overall.as_str(),
                "headline": st.headline,
                "counts": st.counts,
                "repos": st.project.repos.iter().map(|r| serde_json::json!({
                    "id": r.id, "path": r.path.as_str(), "branch": r.head.as_ref().and_then(|h| h.branch.clone()),
                    "head": r.head.as_ref().map(|h| h.sha.as_str().to_string()), "dirty": r.head.as_ref().map(|h| h.dirty).unwrap_or(false),
                    "commits": r.commit_count, "note": r.note,
                })).collect::<Vec<_>>(),
                "attention": st.attention.iter().take(limit).collect::<Vec<_>>(),
                "recent_commits": st.recent_commits.iter().take(limit).filter_map(|k| st.commits.get(k)).map(|c| serde_json::json!({
                    "repo": c.commit.key.repo, "sha": c.commit.key.sha, "short": c.commit.short(), "subject": c.commit.subject,
                    "at": c.commit.committed_at.to_string(),
                    "features": c.features.iter().map(|(f, o)| serde_json::json!({"id": f, "origin": o.key()})).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "adrs": st.adrs.values().map(|a| serde_json::json!({"id": a.adr.id, "title": a.adr.title, "status": a.adr.status.as_str(), "superseded_by": a.adr.superseded_by})).collect::<Vec<_>>(),
                "warnings": st.warnings,
            }),
        );
        return Ok(0);
    }

    // Header.
    let repo_bits: Vec<String> = st
        .project
        .repos
        .iter()
        .map(|r| match &r.head {
            Some(h) => format!(
                "{}{} · {}{}",
                if st.project.repos.len() > 1 { format!("{} ", r.id) } else { String::new() },
                h.branch.clone().unwrap_or_else(|| "detached".into()),
                h.sha.short(),
                if h.dirty {
                    format!(" · dirty ({} files)", h.dirty_files)
                } else {
                    " · clean".into()
                }
            ),
            None => format!("{} skipped", r.id),
        })
        .collect();
    let scanned = st
        .last_scan_at
        .map(|t| format!("scanned {}", ago(t, t_now)))
        .unwrap_or_else(|| "not scanned".into());
    println!("{}    {}", fmt.bold(&st.project.name), fmt.dim(&scanned));
    if st.project.repos.len() <= 3 {
        for b in &repo_bits {
            println!("{}", fmt.dim(b));
        }
    } else {
        let active = st.project.repos.iter().filter(|r| r.head.is_some()).count();
        let skipped: Vec<String> = st
            .project
            .repos
            .iter()
            .filter(|r| r.head.is_none())
            .map(|r| r.id.to_string())
            .collect();
        println!(
            "{}",
            fmt.dim(&format!(
                "{} repos · {}{}",
                active,
                repo_bits.iter().take(3).cloned().collect::<Vec<_>>().join(" · "),
                if skipped.is_empty() {
                    String::new()
                } else {
                    format!(" · {} skipped: {}", skipped.len(), skipped.join(", "))
                }
            ))
        );
    }
    println!("{} — {}", fmt.glyph(st.overall), st.headline);
    println!();

    // Summary rows.
    let cnt = &st.counts;
    println!(
        "{:<13} {} healthy   {} attention   {} broken   {} unknown   {}",
        "Features",
        fmt.paint("32", &thousands(cnt.healthy)),
        fmt.paint("33", &thousands(cnt.attention)),
        fmt.paint("31", &thousands(cnt.broken)),
        fmt.paint("90", &thousands(cnt.unknown)),
        fmt.dim(&format!("{} total", thousands(cnt.features)))
    );
    if cnt.tests > 0 {
        println!(
            "{:<13} {} passed   {} failed   {} flaky   {} skipped",
            "Tests",
            thousands(cnt.tests_passed),
            fmt.paint("31", &thousands(cnt.tests_failed)),
            fmt.paint("33", &thousands(cnt.tests_flaky)),
            thousands(cnt.tests_skipped)
        );
    } else {
        println!("{:<13} {}", "Tests", fmt.dim("no test evidence yet"));
    }
    if cnt.benchmarks > 0 {
        println!(
            "{:<13} {} metrics   {} regressions   {} budget breaches",
            "Performance", cnt.benchmarks, cnt.benchmark_regressions, cnt.benchmark_breaches
        );
    } else {
        println!("{:<13} {}", "Performance", fmt.dim("no benchmark evidence yet"));
    }
    println!(
        "{:<13} {} · {} accepted · {} proposed",
        "ADRs",
        thousands(cnt.adrs),
        cnt.adrs_accepted,
        cnt.adrs_proposed
    );
    println!(
        "{:<13} {} commits{}",
        "Git",
        thousands(cnt.commits),
        if cnt.warnings > 0 {
            fmt.dim(&format!("   · {} import warnings", cnt.warnings))
        } else {
            String::new()
        }
    );
    println!();

    // Attention.
    if st.attention.is_empty() {
        println!("{}", fmt.dim("Nothing needs attention."));
    } else {
        println!("{}", fmt.bold("Needs attention"));
        for a in st.attention.iter().take(limit) {
            let state = a
                .feature
                .as_ref()
                .and_then(|f| st.features.get(f))
                .map(|f| f.health.state)
                .unwrap_or(HealthState::Attention);
            let subject = match (&a.feature, &a.feature_title) {
                (Some(id), Some(title)) => format!("{:<12} {}", id.as_str(), title),
                _ => a.subject.clone(),
            };
            println!("  {}  {:<40} {}", fmt.icon(state), truncate(&subject, 40), a.summary);
        }
        if st.attention.len() > limit {
            println!(
                "  {}",
                fmt.dim(&format!("… {} more (cockpit attention)", st.attention.len() - limit))
            );
        }
    }
    println!();

    // Recent changes.
    println!("{}", fmt.bold("Recent changes"));
    for k in st.recent_commits.iter().take(limit) {
        let Some(c) = st.commits.get(k) else { continue };
        let feats: Vec<String> = c
            .features
            .iter()
            .map(|(f, o)| if o.is_explicit() { f.to_string() } else { format!("{f} (path)") })
            .collect();
        let repo = if st.project.repos.len() > 1 {
            format!("{} ", c.commit.key.repo)
        } else {
            String::new()
        };
        println!(
            "  {:>9}  {}{}  {:<50} {}",
            fmt.dim(&ago(c.commit.committed_at, t_now)),
            fmt.dim(&repo),
            c.commit.short(),
            truncate(&c.commit.subject, 50),
            fmt.dim(&feats.join(", "))
        );
    }
    println!();
    println!("{}", fmt.dim("cockpit attention · cockpit serve"));
    Ok(0)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}
