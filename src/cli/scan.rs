use super::output::{Format, thousands};
use crate::cockpit::Cockpit;
use crate::scan::ScanProgress;
use std::sync::Mutex;

pub fn run(c: &mut Cockpit, full: bool, reset_history: bool, fmt: &Format) -> anyhow::Result<i32> {
    if full {
        if reset_history { c.store.clear_all()? } else { c.store.clear_cache()? }
    }
    let name = c.located.project_name();
    let repo_count = crate::scan::discover_repos(&c.located).len();
    if !fmt.json {
        println!(
            "Scanning {} ({} repo{}) …",
            fmt.bold(&name),
            repo_count,
            if repo_count == 1 { "" } else { "s" }
        );
    }
    let last_phase = Mutex::new(String::new());
    let progress = |p: ScanProgress| {
        if fmt.json {
            return;
        }
        if let ScanProgress::Phase(ph) = p {
            *last_phase.lock().unwrap() = ph.to_string();
        }
    };
    let report = c.scan(Some(&progress))?;
    let st = c.state.load();
    if fmt.json {
        fmt.json_out(
            "cockpit.scan.v1",
            &st.project,
            serde_json::json!({
                "duration_ms": report.duration_ms,
                "counts": {
                    "repos": report.counts.repos, "commits": report.counts.commits, "new_commits": report.counts.new_commits,
                    "features": report.counts.features, "adrs": report.counts.adrs, "files_seen": report.counts.files_seen,
                    "files_imported": report.counts.files_imported, "warnings": report.counts.warnings,
                },
                "repo_notes": report.repo_notes.iter().map(|(r, n)| serde_json::json!({"repo": r, "note": n})).collect::<Vec<_>>(),
                "warnings": report.warnings,
            }),
        );
        return Ok(0);
    }
    let repos_line = st
        .project
        .repos
        .iter()
        .filter_map(|r| {
            r.head.as_ref().map(|h| {
                format!("{}@{}{}", r.id, h.sha.short(), if h.dirty { " dirty" } else { "" })
            })
        })
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "  {:<12} {:>7} ({} new)   {}",
        "Git",
        thousands(report.counts.commits),
        report.counts.new_commits,
        fmt.dim(&repos_line)
    );
    for (repo, note) in &report.repo_notes {
        println!("  {:<12} {:>7} {}", "", "", fmt.dim(&format!("{repo}: {note}")));
    }
    println!(
        "  {:<12} {:>7}   {}",
        "Features",
        thousands(report.counts.features),
        fmt.dim(&c.located.config.features.path)
    );
    println!(
        "  {:<12} {:>7}   {}",
        "ADRs",
        thousands(report.counts.adrs),
        fmt.dim(&c.located.config.adrs.path.items().join(", "))
    );
    let secs = report.duration_ms as f64 / 1000.0;
    let w = report.counts.warnings;
    println!(
        "Indexed in {secs:.1} s · {} file{} read{}",
        report.counts.files_imported,
        if report.counts.files_imported == 1 { "" } else { "s" },
        if w > 0 {
            format!(" · {} import warning{} (cockpit doctor)", w, if w == 1 { "" } else { "s" })
        } else {
            String::new()
        }
    );
    Ok(0)
}
