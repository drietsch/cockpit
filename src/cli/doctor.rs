use super::output::Format;
use crate::cockpit::Cockpit;
use crate::paths::RelPath;

pub fn run(c: &Cockpit, fmt: &Format) -> anyhow::Result<i32> {
    let mut problems = 0;
    let mut rows: Vec<(char, String)> = Vec::new();
    let ok = |rows: &mut Vec<(char, String)>, s: String| rows.push(('✓', s));
    let warn = |rows: &mut Vec<(char, String)>, s: String| rows.push(('⚠', s));
    let bad = |rows: &mut Vec<(char, String)>, s: String, problems: &mut i32| {
        rows.push(('✕', s));
        *problems += 1;
    };

    ok(&mut rows, format!("project root {}", c.located.root.display()));
    match &c.located.config_path {
        Some(p) => ok(&mut rows, format!("config {}", p.display())),
        None => warn(
            &mut rows,
            "no cockpit.toml — using conventions (cockpit init to scaffold one)".into(),
        ),
    }
    match c.git.version() {
        Ok(v) => ok(&mut rows, v),
        Err(e) => bad(&mut rows, format!("git: {e}"), &mut problems),
    }
    let repos = crate::scan::discover_repos(&c.located);
    for r in &repos {
        let abs = r.path.to_abs(&c.located.root);
        match c.git.head(&abs, true) {
            Ok(Some(h)) => {
                let count = c.git.commit_count(&abs).unwrap_or(0);
                let branch = h.branch.clone().unwrap_or_else(|| "detached".into());
                let primary = r
                    .primary_branch
                    .clone()
                    .unwrap_or_else(|| c.located.config.project.primary_branch.clone());
                let mut line = format!(
                    "repo {} · {} · {} · {} commits{}",
                    r.id,
                    branch,
                    &h.sha[..7.min(h.sha.len())],
                    count,
                    if h.dirty_files > 0 {
                        format!(" · {} dirty", h.dirty_files)
                    } else {
                        String::new()
                    }
                );
                if branch != primary && h.branch.is_some() {
                    line.push_str(&format!(" · not on {primary}"));
                    warn(&mut rows, line);
                } else {
                    ok(&mut rows, line);
                }
            }
            Ok(None) => warn(&mut rows, format!("repo {} has no commits — skipped", r.id)),
            Err(e) => bad(&mut rows, format!("repo {}: {e}", r.id), &mut problems),
        }
    }
    let fdir = RelPath::new(&c.located.config.features.path).to_abs(&c.located.root);
    let features = crate::scan::feature_files(&c.located);
    if fdir.is_dir() {
        ok(
            &mut rows,
            format!("features {} · {} files", c.located.config.features.path, features.len()),
        );
    } else {
        warn(&mut rows, format!("features dir {} does not exist", c.located.config.features.path));
    }
    let adrs = crate::scan::adr_files(&c.located);
    let adr_dirs = c.located.config.adrs.path.items();
    let any_adr_dir = adr_dirs.iter().any(|d| RelPath::new(d).to_abs(&c.located.root).is_dir());
    if any_adr_dir {
        ok(&mut rows, format!("adrs {} · {} files", adr_dirs.join(", "), adrs.len()));
    } else {
        warn(&mut rows, format!("adr dir {} does not exist", adr_dirs.join(", ")));
    }
    for src in c.located.evidence_sources() {
        let id = src.id.clone().unwrap_or_else(|| format!("auto:{}", src.format.as_str()));
        let mut matched = 0usize;
        let mut newest: Option<jiff::Timestamp> = None;
        for pat in src.paths() {
            for p in crate::scan::glob_files(&c.located.root, &pat) {
                matched += 1;
                if let Ok(m) = std::fs::metadata(&p) {
                    if let Some(t) =
                        m.modified().ok().and_then(|t| jiff::Timestamp::try_from(t).ok())
                    {
                        newest = Some(newest.map_or(t, |n: jiff::Timestamp| n.max(t)));
                    }
                }
            }
        }
        let gh = src
            .github
            .as_ref()
            .map(|g| format!(" · github {}/{} {}", g.repo, g.workflow, g.artifact))
            .unwrap_or_default();
        if matched > 0 {
            ok(
                &mut rows,
                format!(
                    "evidence {id} ({}) · {} file{} · newest {}{gh}",
                    src.format.as_str(),
                    matched,
                    if matched == 1 { "" } else { "s" },
                    newest
                        .map(|t| crate::time::ago(t, crate::time::now()))
                        .unwrap_or_else(|| "?".into())
                ),
            );
        } else if src.enabled && !src.paths().is_empty() && src.id.is_some() {
            warn(
                &mut rows,
                format!(
                    "evidence {id} ({}) · no files match {}{gh}",
                    src.format.as_str(),
                    src.paths().join(", ")
                ),
            );
        }
    }
    match c.store.integrity_ok() {
        Ok(true) => ok(
            &mut rows,
            format!("index {} · {} KB", c.store.path.display(), c.store.size_bytes() / 1024),
        ),
        Ok(false) => bad(
            &mut rows,
            format!(
                "index {} failed integrity check — delete it and rescan",
                c.store.path.display()
            ),
            &mut problems,
        ),
        Err(e) => bad(&mut rows, format!("index: {e}"), &mut problems),
    }
    let warnings = c.store.warnings().unwrap_or_default();
    if warnings.is_empty() {
        ok(&mut rows, "no import warnings".into());
    } else {
        warn(
            &mut rows,
            format!(
                "{} import warning{}",
                warnings.len(),
                if warnings.len() == 1 { "" } else { "s" }
            ),
        );
        for w in warnings.iter().take(20) {
            rows.push((
                ' ',
                format!(
                    "   {} — {}{}",
                    w.path,
                    w.message,
                    w.location.as_ref().map(|l| format!(" ({l})")).unwrap_or_default()
                ),
            ));
        }
    }

    if fmt.json {
        let st = c.state.load();
        fmt.json_out(
            "cockpit.doctor.v1",
            &st.project,
            serde_json::json!({ "problems": problems, "checks": rows.iter().map(|(g, s)| serde_json::json!({"level": match g { '✓' => "ok", '⚠' => "warn", '✕' => "error", _ => "detail" }, "text": s})).collect::<Vec<_>>() }),
        );
    } else {
        for (g, s) in &rows {
            let icon = match g {
                '✓' => fmt.paint("32", if fmt.ascii { "OK  " } else { "✓" }),
                '⚠' => fmt.paint("33", if fmt.ascii { "WARN" } else { "⚠" }),
                '✕' => fmt.paint("31", if fmt.ascii { "FAIL" } else { "✕" }),
                _ => " ".into(),
            };
            println!("{icon} {s}");
        }
    }
    Ok(if problems > 0 { 1 } else { 0 })
}
