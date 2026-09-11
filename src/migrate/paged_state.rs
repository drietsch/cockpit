//! `cockpit migrate paged-state <state-dir>`: turn a paged-media/state registry into feature
//! Markdown files plus `cockpit.toml` fragments. Lossless and idempotent — every registry field
//! lands somewhere reviewable, hand-edited files are never overwritten without `--force`, and a
//! second run produces byte-identical output.

use crate::cli::output::Format;
use crate::config::Located;
use crate::hash::ContentHash;
use crate::model::canonical_json;
use serde_yaml_ng::{Mapping, Value as Y};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const BEGIN_MARK: &str = "# >>> cockpit migrate (paged-state) >>>";
const END_MARK: &str = "# <<< end cockpit migrate (paged-state) <<<";

#[derive(Debug, Default)]
pub struct Report {
    pub features_total: usize,
    pub by_chapter: BTreeMap<String, (usize, usize)>, // (registry rows, files written/unchanged)
    pub by_status: BTreeMap<String, usize>,
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub kept_body: usize,
    pub skipped_hand_authored: Vec<String>,
    pub components: usize,
    pub evidence: usize,
    pub evidence_unfed: usize,
    pub splits: usize,
    pub test_map_entries: usize,
    pub test_map_unknown_features: Vec<String>,
    pub metrics: usize,
    pub warnings: Vec<String>,
    pub not_migrated: Vec<String>,
}

pub fn run(
    state_dir: &Path,
    out: Option<&Path>,
    config_out: Option<&Path>,
    located: Option<&Located>,
    dry_run: bool,
    force: bool,
    fmt: &Format,
) -> anyhow::Result<i32> {
    let state_dir = state_dir
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("state dir {}: {e}", state_dir.display()))?;
    let registry = state_dir.join("registry");
    if !registry.join("features").is_dir() {
        anyhow::bail!(
            "{} does not look like a paged-media/state checkout (no registry/features)",
            state_dir.display()
        );
    }
    let config_out: PathBuf = match config_out {
        Some(p) => p.to_path_buf(),
        None => match located.and_then(|l| l.config_path.clone()) {
            Some(p) => p,
            None => std::env::current_dir()?.join("cockpit.toml"),
        },
    };
    let config_dir =
        config_out.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    let out_dir: PathBuf =
        out.map(|p| p.to_path_buf()).unwrap_or_else(|| config_dir.join("docs").join("features"));
    let umbrella = state_dir.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| state_dir.clone());

    let mut report = Report::default();
    let chapters = load_chapters(&registry.join("chapters.yaml"))?;
    let stages = load_list(&registry.join("stages.yaml"))?;
    let rows = load_features(&registry.join("features"), &mut report)?;
    report.features_total = rows.len();

    // ── Feature files ─────────────────────────────────────────────────────
    let mut planned_writes: Vec<(PathBuf, String)> = Vec::new();
    for (row, src_file) in &rows {
        let id = str_of(row, "id").unwrap_or_default();
        let chapter = id.split('.').next().unwrap_or("misc").to_string();
        let entry = report.by_chapter.entry(chapter.clone()).or_insert((0, 0));
        entry.0 += 1;
        let (fm, body, status) = feature_document(row, &chapter, &chapters, src_file);
        *report.by_status.entry(status).or_insert(0) += 1;
        let target = out_dir.join(&chapter).join(format!("{id}.md"));
        let new_text = format!("---\n{fm}---\n{body}");
        match std::fs::read_to_string(&target) {
            Ok(existing) => {
                let existing_hash = migrated_hash_of(&existing);
                let new_hash = migrated_hash_of(&new_text);
                if existing_hash.is_none() && !force {
                    report.skipped_hand_authored.push(rel(&target, &config_dir));
                    continue;
                }
                if existing_hash == new_hash && existing == new_text {
                    report.unchanged += 1;
                    entry.1 += 1;
                    continue;
                }
                // Frontmatter changed: keep a hand-edited body.
                let existing_body = body_of(&existing);
                let final_text = if existing_body.trim() != body_of(&new_text).trim()
                    && !existing_body.trim().is_empty()
                    && existing_hash != new_hash
                {
                    report.kept_body += 1;
                    format!("---\n{fm}---\n{}", existing_body)
                } else {
                    new_text
                };
                if final_text == existing {
                    report.unchanged += 1;
                } else {
                    report.updated += 1;
                    planned_writes.push((target, final_text));
                }
                entry.1 += 1;
            }
            Err(_) => {
                report.created += 1;
                entry.1 += 1;
                planned_writes.push((target, new_text));
            }
        }
    }
    for ch in &chapters {
        let target = out_dir.join(&ch.id).join("_chapter.md");
        let text = format!(
            "---\ntitle: {}\ngroup: {}\norder: {}\n---\n# {}\n\n{}",
            yaml_str(&ch.title),
            yaml_str(&ch.group),
            ch.order,
            ch.title,
            ch.note.as_deref().map(|n| format!("{n}\n")).unwrap_or_default()
        );
        if std::fs::read_to_string(&target).ok().as_deref() != Some(&text) {
            planned_writes.push((target, text));
        }
    }

    // ── Config fragments ──────────────────────────────────────────────────
    let suites = load_list(&state_dir.join("ingest").join("suites.yaml")).unwrap_or_default();
    let artifacts = load_list(&state_dir.join("ingest").join("artifacts.yaml")).unwrap_or_default();
    let measurements_reg =
        load_mapping_list(&registry.join("measurements.yaml"), "measurements").unwrap_or_default();
    let measurements_ingest =
        load_mapping_list(&state_dir.join("ingest").join("measurements.yaml"), "measurements")
            .unwrap_or_default();
    let mut frag = String::new();
    frag.push_str(BEGIN_MARK);
    frag.push_str("\n# Generated from paged-media/state; re-run `cockpit migrate paged-state` to refresh this block.\n");
    frag.push_str("# Lines outside the markers are yours.\n\n");
    for st in &stages {
        let id = str_of(st, "id").unwrap_or_default();
        let group = id.split('.').next().unwrap_or("").to_string();
        frag.push_str(&format!(
            "[[components]]\nid = {}\ntitle = {}\ngroup = {}\norder = {}\n\n",
            toml_str(&id),
            toml_str(&str_of(st, "title").unwrap_or_else(|| id.clone())),
            toml_str(&group),
            int_of(st, "order").unwrap_or(0)
        ));
        report.components += 1;
    }
    let mut evidence_ids: Vec<String> = Vec::new();
    for s in &suites {
        let source = str_of(s, "source").unwrap_or_default();
        let suite = str_of(s, "suite").unwrap_or_default();
        let id = format!("{source}/{suite}");
        let stage = str_of(s, "stage");
        let display = str_of(s, "display");
        let core_pull = core_pull_artifact(&source, &suite);
        let art = artifacts
            .iter()
            .find(|a| {
                str_of(a, "source").as_deref() == Some(&source)
                    && str_of(a, "suite").as_deref() == Some(&suite)
            })
            .or(core_pull.as_ref());
        let art_format = art.and_then(|a| str_of(a, "format"));
        let (format, kind) = classify_suite(&source, &suite, art_format.as_deref());
        let local =
            local_paths(&source, &suite, &format, art.and_then(|a| str_of(a, "repo")).as_deref());
        let pulled = format!(".cockpit/evidence/{}/**", id.replace('/', "__"));
        let mut paths: Vec<String> = vec![pulled];
        paths.extend(local.clone());
        let unfed = art.is_none() && local.is_empty();
        if unfed {
            report.evidence_unfed += 1;
        }
        frag.push_str("[[evidence]]\n");
        frag.push_str(&format!("id = {}\n", toml_str(&id)));
        if let Some(d) = display {
            frag.push_str(&format!("title = {}\n", toml_str(&d)));
        }
        frag.push_str(&format!("format = {}\nkind = {}\n", toml_str(&format), toml_str(&kind)));
        frag.push_str(&format!(
            "path = [{}]\n",
            paths.iter().map(|p| toml_str(p)).collect::<Vec<_>>().join(", ")
        ));
        if let Some(st) = &stage {
            frag.push_str(&format!("component = {}\n", toml_str(st)));
        }
        if let Some(repo_dir) =
            repo_dir_for(&source, art.and_then(|a| str_of(a, "repo")).as_deref())
        {
            frag.push_str(&format!("repo = {}\n", toml_str(&repo_dir)));
        }
        if suite == "conformance" {
            frag.push_str("[evidence.component_by_level]\nparsed = \"core.parser\"\nrendered = \"core.renderer\"\nmutatable = \"core.mutation\"\n\"round-trips\" = \"core.mutation\"\n\"round-trips-mutated\" = \"core.mutation\"\n");
        }
        if unfed {
            frag.push_str("enabled = false\nnote = \"unfed in paged-media/state: no CI artifact and no local path known\"\n");
        }
        if let Some(a) = art {
            frag.push_str(&format!(
                "[evidence.github]\nrepo = {}\nworkflow = {}\nartifact = {}\nbranch = \"main\"\n",
                toml_str(&str_of(a, "repo").unwrap_or_default()),
                toml_str(&str_of(a, "workflow").unwrap_or_default()),
                toml_str(&str_of(a, "artifact").unwrap_or_default())
            ));
            if let Some(Y::Sequence(splits)) = a.get(Y::String("split".into())) {
                for sp in splits {
                    let Some(sp) = sp.as_mapping() else { continue };
                    let crates: Vec<String> =
                        sp.get(Y::String("crates".into())).map(list_of).unwrap_or_default();
                    let sid = format!(
                        "{}/{}",
                        str_of(sp, "source").unwrap_or_default(),
                        str_of(sp, "suite").unwrap_or_default()
                    );
                    let comp = suites
                        .iter()
                        .find(|x| {
                            format!(
                                "{}/{}",
                                str_of(x, "source").unwrap_or_default(),
                                str_of(x, "suite").unwrap_or_default()
                            ) == sid
                        })
                        .and_then(|x| str_of(x, "stage"));
                    frag.push_str(&format!(
                        "[[evidence.split]]\npackages = [{}]\nid = {}\n{}",
                        crates.iter().map(|c| toml_str(c)).collect::<Vec<_>>().join(", "),
                        toml_str(&sid),
                        comp.map(|c| format!("component = {}\n", toml_str(&c))).unwrap_or_default()
                    ));
                    report.splits += 1;
                }
            }
        }
        frag.push('\n');
        evidence_ids.push(id);
        report.evidence += 1;
    }
    // core-pull.yml's filter-junit split (declared in suites.yaml without an artifacts entry).
    if evidence_ids.iter().any(|i| i == "core/nextest-unit")
        && evidence_ids.iter().any(|i| i == "core-script/nextest-canvas-wasm")
    {
        frag = frag.replace(
            "id = \"core/nextest-unit\"\n",
            "id = \"core/nextest-unit\"\n# paged-script tests are routed to core-script/nextest-canvas-wasm (see [[evidence.split]] below)\n",
        );
        let split_block = "[[evidence.split]]\npackages = [\"paged-script\"]\nid = \"core-script/nextest-canvas-wasm\"\ncomponent = \"core.canvas-wasm\"\n";
        if let Some(pos) = frag.find("id = \"core/nextest-unit\"\n") {
            if let Some(end) = frag[pos..].find("\n\n") {
                frag.insert_str(pos + end + 1, split_block);
                report.splits += 1;
            }
        }
    }
    for m in &measurements_ingest {
        let source = str_of(m, "source").unwrap_or_default();
        let lane = str_of(m, "lane").unwrap_or_default();
        let id = format!("{source}/{lane}");
        frag.push_str(&format!(
            "[[evidence]]\nid = {}\nformat = \"paged-measurements\"\nkind = \"benchmark\"\npath = [{}]\n{}[evidence.github]\nrepo = {}\nworkflow = {}\nartifact = {}\nbranch = \"main\"\n\n",
            toml_str(&id),
            toml_str(&format!(".cockpit/evidence/{}/**", id.replace('/', "__"))),
            repo_dir_for(&source, str_of(m, "repo").as_deref()).map(|r| format!("repo = {}\n", toml_str(&r))).unwrap_or_default(),
            toml_str(&str_of(m, "repo").unwrap_or_default()),
            toml_str(&str_of(m, "workflow").unwrap_or_default()),
            toml_str(&str_of(m, "artifact").unwrap_or_default())
        ));
        report.evidence += 1;
    }
    for m in &measurements_reg {
        let id = str_of(m, "id").unwrap_or_default();
        let unit = str_of(m, "unit").unwrap_or_default();
        let determinism = match unit.as_str() {
            "bytes" | "count" | "B" => "deterministic",
            "de2000" | "ssim" => "semi-deterministic",
            _ => "environment-sensitive",
        };
        frag.push_str(&format!(
            "[[performance.metrics]]\nmatch = {}\nid = {}\n{}unit = {}\ndirection = {}\ndeterminism = {}\n{}{}{}{}\n",
            toml_str(&format!("*/*/{id}")),
            toml_str(&id),
            str_of(m, "title").map(|t| format!("title = {}\n", toml_str(&t))).unwrap_or_default(),
            toml_str(&unit),
            toml_str(&str_of(m, "direction").unwrap_or_else(|| "lower_is_better".into())),
            toml_str(determinism),
            str_of(m, "gate").map(|g| format!("gate = {}\n", toml_str(&g))).unwrap_or_default(),
            m.get(Y::String("tolerance_pct".into())).and_then(|v| v.as_f64()).map(|t| format!("tolerance_pct = {t}\n")).unwrap_or_default(),
            m.get(Y::String("subjects".into())).map(|v| format!("subjects = [{}]\n", list_of(v).iter().map(|s| toml_str(s)).collect::<Vec<_>>().join(", "))).unwrap_or_default(),
            str_of(m, "why_not_evidence").map(|n| format!("note = {}\n", toml_str(n.trim()))).unwrap_or_default(),
        ));
        report.metrics += 1;
    }
    frag.push_str(END_MARK);
    frag.push('\n');

    // Test map: copied verbatim with a header.
    let test_map_src = state_dir.join("ingest").join("test-map.yaml");
    let test_map_target = config_dir.join("test-map.yaml");
    let mut test_map_text: Option<String> = None;
    if let Ok(text) = std::fs::read_to_string(&test_map_src) {
        let entries: Result<Vec<crate::config::TestMapEntry>, _> = serde_yaml_ng::from_str(&text);
        match entries {
            Ok(entries) => {
                report.test_map_entries = entries.len();
                let known: std::collections::HashSet<String> =
                    rows.iter().filter_map(|(r, _)| str_of(r, "id")).collect();
                for e in &entries {
                    for f in &e.features {
                        if !known.contains(f) && !report.test_map_unknown_features.contains(f) {
                            report.test_map_unknown_features.push(f.clone());
                        }
                    }
                }
                let header = "# Migrated verbatim from paged-media/state ingest/test-map.yaml by `cockpit migrate paged-state`.\n# Schema: match {file|title|ac_prefix|source}, features[], component (alias stage), level (alias coverage_level).\n".to_string();
                let body = text.trim_start_matches('\u{feff}');
                let new_text = format!("{header}{body}");
                if std::fs::read_to_string(&test_map_target).ok().as_deref() != Some(&new_text) {
                    test_map_text = Some(new_text);
                }
            }
            Err(e) => report
                .warnings
                .push(format!("test-map.yaml does not parse under the strict schema: {e}")),
        }
    }

    // Assemble the config file text.
    let existing_cfg = std::fs::read_to_string(&config_out).unwrap_or_default();
    let features_root_rel = pathdiff(&umbrella, &out_dir);
    let new_cfg = splice_config(
        &existing_cfg,
        &frag,
        &features_root_rel,
        &umbrella,
        &config_dir,
        test_map_text.is_some() || test_map_target.exists(),
    );

    report.not_migrated = vec![
        "registry/artifacts.yaml (claims about showcase artifacts — Cockpit shows claims only as feature `claims:`)".into(),
        "data/bugs.json (failure transitions are derived from run history from the first scan on)".into(),
        "ingest/schedules.yaml (test cadence belongs to the producing repos)".into(),
        "registry/triage.yaml, plans, dashboard/, data/*.json, STATUS.md (generated or tool-specific)".into(),
    ];

    // ── Apply ─────────────────────────────────────────────────────────────
    if !dry_run {
        for (path, text) in &planned_writes {
            if let Some(d) = path.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(path, text)?;
        }
        if new_cfg != existing_cfg {
            if let Some(d) = config_out.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(&config_out, &new_cfg)?;
        }
        if let Some(t) = &test_map_text {
            std::fs::write(&test_map_target, t)?;
        }
    }

    // ── Report ────────────────────────────────────────────────────────────
    let files_total: usize = report.by_chapter.values().map(|(_, w)| w).sum();
    let ok_features = files_total + report.skipped_hand_authored.len() == report.features_total;
    let ok_suites = report.evidence >= suites.len();
    let ok_map = report.test_map_unknown_features.is_empty();
    if fmt.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "cockpit.migrate.v1",
                "dry_run": dry_run,
                "features": { "registry": report.features_total, "files": files_total, "created": report.created, "updated": report.updated,
                              "unchanged": report.unchanged, "kept_body": report.kept_body, "skipped_hand_authored": report.skipped_hand_authored,
                              "by_status": report.by_status, "by_chapter": report.by_chapter.iter().map(|(k, (a, b))| (k.clone(), serde_json::json!({"registry": a, "files": b}))).collect::<serde_json::Map<_, _>>() },
                "components": report.components, "evidence": report.evidence, "evidence_unfed": report.evidence_unfed, "splits": report.splits,
                "test_map": { "entries": report.test_map_entries, "unknown_features": report.test_map_unknown_features },
                "metrics": report.metrics, "warnings": report.warnings, "not_migrated": report.not_migrated,
                "outputs": { "features_dir": out_dir, "config": config_out, "test_map": test_map_target },
                "ok": ok_features && ok_suites && ok_map,
            }))?
        );
    } else {
        println!(
            "{}",
            fmt.bold(&format!(
                "cockpit migrate paged-state{}",
                if dry_run { " (dry run)" } else { "" }
            ))
        );
        println!("  registry   {}", state_dir.display());
        println!("  features → {}", out_dir.display());
        println!("  config   → {}", config_out.display());
        println!("  test map → {}", test_map_target.display());
        println!();
        println!("{}", fmt.bold("Features by chapter"));
        for (ch, (a, b)) in &report.by_chapter {
            println!("  {:<34} {:>4} rows  {:>4} files", ch, a, b);
        }
        println!();
        println!("{}", fmt.bold("Status derivation"));
        for (st, n) in &report.by_status {
            println!("  {:<14} {:>4}", st, n);
        }
        println!("  {}", fmt.dim("deprecated = lifecycle deprecated or superseded_by; planned = no stage shipped/partial; stable = any stage shipped; experimental = partial only"));
        println!();
        println!(
            "Files: {} created · {} updated · {} unchanged · {} kept hand-edited body{}",
            report.created,
            report.updated,
            report.unchanged,
            report.kept_body,
            if report.skipped_hand_authored.is_empty() {
                String::new()
            } else {
                format!(
                    " · {} hand-authored skipped (use --force)",
                    report.skipped_hand_authored.len()
                )
            }
        );
        println!(
            "Config: {} components · {} evidence sources ({} unfed, {} splits) · {} performance metrics · test map {} entries",
            report.components,
            report.evidence,
            report.evidence_unfed,
            report.splits,
            report.metrics,
            report.test_map_entries
        );
        for w in &report.warnings {
            println!("  {} {w}", fmt.paint("33", "warning:"));
        }
        println!();
        println!("{}", fmt.bold("Not migrated (by design)"));
        for n in &report.not_migrated {
            println!("  · {n}");
        }
        println!();
        let check = |ok: bool| if ok { fmt.paint("32", "✓") } else { fmt.paint("31", "✕") };
        println!(
            "registry rows {} = files {} {} · suites {} → evidence {} {} · test-map unknown feature ids: {} {}",
            report.features_total,
            files_total + report.skipped_hand_authored.len(),
            check(ok_features),
            suites.len(),
            report.evidence,
            check(ok_suites),
            report.test_map_unknown_features.len(),
            check(ok_map)
        );
        if !ok_map {
            println!("  {}", fmt.dim(&report.test_map_unknown_features.join(", ")));
        }
    }
    Ok(if ok_features && ok_suites && ok_map { 0 } else { 1 })
}

// ── Feature document ───────────────────────────────────────────────────────

struct Chapter {
    id: String,
    title: String,
    group: String,
    order: usize,
    note: Option<String>,
}

fn feature_document(
    row: &Mapping,
    chapter: &str,
    chapters: &[Chapter],
    src_file: &str,
) -> (String, String, String) {
    let id = str_of(row, "id").unwrap_or_default();
    let title = str_of(row, "title").unwrap_or_else(|| id.clone());
    let lifecycle = str_of(row, "lifecycle");
    let superseded_by = str_of(row, "superseded_by");
    let stages: Vec<(String, String, Option<String>)> = match row.get(Y::String("stages".into())) {
        Some(Y::Mapping(m)) => m
            .iter()
            .filter_map(|(k, v)| {
                let comp = k.as_str()?.to_string();
                let (status, note) = match v {
                    Y::Mapping(inner) => (str_of(inner, "status")?, str_of(inner, "note")),
                    other => (other.as_str()?.to_string(), None),
                };
                Some((comp, status, note))
            })
            .collect(),
        _ => vec![],
    };
    let status = if lifecycle.as_deref() == Some("deprecated") || superseded_by.is_some() {
        "deprecated"
    } else if stages.is_empty() || stages.iter().all(|(_, s, _)| s == "planned") {
        "planned"
    } else if stages.iter().any(|(_, s, _)| s == "shipped") {
        "stable"
    } else {
        "experimental"
    };
    let group = chapters
        .iter()
        .find(|c| c.id == chapter)
        .map(|c| c.group.clone())
        .unwrap_or_else(|| "misc".into());

    let mut fm = Mapping::new();
    fm.insert(Y::String("id".into()), Y::String(id.clone()));
    fm.insert(Y::String("title".into()), Y::String(title.clone()));
    fm.insert(Y::String("status".into()), Y::String(status.into()));
    fm.insert(Y::String("importance".into()), Y::String("normal".into()));
    fm.insert(
        Y::String("tags".into()),
        Y::Sequence(vec![Y::String(chapter.into()), Y::String(group)]),
    );
    if !stages.is_empty() {
        let mut claims = Mapping::new();
        for (comp, st, note) in &stages {
            let mut c = Mapping::new();
            c.insert(Y::String("status".into()), Y::String(st.clone()));
            if let Some(n) = note {
                c.insert(Y::String("note".into()), Y::String(n.clone()));
            }
            claims.insert(Y::String(comp.clone()), Y::Mapping(c));
        }
        fm.insert(Y::String("claims".into()), Y::Mapping(claims));
    }
    if let Some(spec) = str_of(row, "spec") {
        fm.insert(Y::String("references".into()), Y::Sequence(vec![Y::String(spec)]));
    }
    if let Some(since) = str_of(row, "since") {
        fm.insert(Y::String("since".into()), Y::String(since));
    }
    if let Some(s) = &superseded_by {
        fm.insert(Y::String("superseded_by".into()), Y::String(s.clone()));
    }
    let mut metadata = Mapping::new();
    for key in ["in_progress_since", "corpus_scope", "coverage_scope"] {
        if let Some(v) = row.get(Y::String(key.into())) {
            metadata.insert(Y::String(key.into()), v.clone());
        }
    }
    metadata.insert(Y::String("migrated_from".into()), Y::String(src_file.to_string()));
    let hash =
        ContentHash::of(canonical_json(&serde_json::to_value(row).unwrap_or_default()).as_bytes())
            .short();
    metadata.insert(Y::String("migrated_hash".into()), Y::String(hash));
    fm.insert(Y::String("metadata".into()), Y::Mapping(metadata));

    let fm_text = serde_yaml_ng::to_string(&Y::Mapping(fm)).unwrap_or_default();
    let notes = str_of(row, "notes").unwrap_or_default();
    let body = if notes.trim().is_empty() {
        format!("# {title}\n")
    } else {
        format!("# {title}\n\n{}\n", notes.trim())
    };
    (fm_text, body, status.into())
}

fn migrated_hash_of(text: &str) -> Option<String> {
    let split = crate::import::frontmatter::split(text);
    let yaml = split.yaml?;
    let m = crate::import::frontmatter::parse_yaml(yaml).ok()?;
    let meta = m.get(Y::String("metadata".into()))?;
    meta.get(Y::String("migrated_hash".into()))?.as_str().map(|s| s.to_string())
}

fn body_of(text: &str) -> String {
    crate::import::frontmatter::split(text).body.to_string()
}

// ── Config splicing ─────────────────────────────────────────────────────────

fn splice_config(
    existing: &str,
    frag: &str,
    features_root_rel: &str,
    umbrella: &Path,
    config_dir: &Path,
    has_test_map: bool,
) -> String {
    if let (Some(b), Some(e)) = (existing.find(BEGIN_MARK), existing.find(END_MARK)) {
        let end = e + END_MARK.len();
        let end = if existing[end..].starts_with('\n') { end + 1 } else { end };
        return format!("{}{}{}", &existing[..b], frag, &existing[end..]);
    }
    let mut head = existing.to_string();
    if head.trim().is_empty() {
        let root = pathdiff(config_dir, umbrella);
        head = format!(
            "[project]\nid = \"paged\"\nname = \"Paged\"\nroot = {}\n\n[git]\n# paged-media/state is the system this migration replaces; its ~1,100 regenerate commits are noise.\nskip_repos = [\"state\"]\n\n[features]\npath = {}\nid_pattern = \"^[a-z0-9-]+(\\\\.[a-z0-9-]+)+$\"\n\n[adrs]\npath = \"thoughts/docs/paged/adr\"\n\n{}[commits]\nid_patterns = [\"ADR \\\\d{{3}}\", \"\\\\b[A-Z]-\\\\d{{2}}\\\\b\", \"W\\\\d+\\\\.\\\\d+\"]\n\n",
            toml_str(&root),
            toml_str(features_root_rel),
            if has_test_map { "[test_map]\nfiles = [\"test-map.yaml\"]\n\n" } else { "" }
        );
    }
    if !head.ends_with('\n') {
        head.push('\n');
    }
    format!("{head}\n{frag}")
}

fn pathdiff(from: &Path, to: &Path) -> String {
    let from = from.canonicalize().unwrap_or_else(|_| from.to_path_buf());
    let to = to.canonicalize().unwrap_or_else(|_| to.to_path_buf());
    if from == to {
        return ".".into();
    }
    if let Ok(r) = to.strip_prefix(&from) {
        return r.to_string_lossy().to_string();
    }
    let mut ups = 0;
    let mut cur = from.clone();
    while !to.starts_with(&cur) {
        match cur.parent() {
            Some(p) => {
                cur = p.to_path_buf();
                ups += 1;
            }
            None => return to.to_string_lossy().to_string(),
        }
    }
    let tail = to.strip_prefix(&cur).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let mut s = vec![".."; ups].join("/");
    if !tail.is_empty() {
        s = format!("{s}/{tail}");
    }
    s
}

fn rel(p: &Path, base: &Path) -> String {
    pathdiff(base, p)
}

// ── Suites → evidence ─────────────────────────────────────────────────────

/// paged-media/state pulls core's JUnit through a dedicated core-pull.yml rather than
/// ingest/artifacts.yaml; the artifact it reads is `nextest-junit` from core's ci.yml.
fn core_pull_artifact(source: &str, suite: &str) -> Option<Mapping> {
    if source != "core" || suite != "nextest-unit" {
        return None;
    }
    let mut m = Mapping::new();
    for (k, v) in [
        ("source", "core"),
        ("suite", "nextest-unit"),
        ("repo", "paged-media/core"),
        ("workflow", "ci.yml"),
        ("artifact", "nextest-junit"),
        ("format", "nextest-junit"),
    ] {
        m.insert(Y::String(k.into()), Y::String(v.into()));
    }
    Some(m)
}

fn classify_suite(source: &str, suite: &str, art_format: Option<&str>) -> (String, String) {
    let kind = if suite.starts_with("playwright") {
        "e2e"
    } else if suite == "conformance" {
        "conformance"
    } else if suite == "fidelity" {
        "visual"
    } else {
        "unit"
    };
    let format = match art_format {
        Some("playwright-json") => "playwright",
        Some("nextest-junit") => "junit",
        Some("contract") => "paged-results",
        _ => {
            if suite == "conformance" {
                "paged-results"
            } else if suite == "fidelity" {
                "fidelity"
            } else if suite.starts_with("playwright") {
                "playwright"
            } else {
                "junit"
            }
        }
    };
    let _ = source;
    (format.into(), kind.into())
}

fn repo_dir_for(source: &str, gh_repo: Option<&str>) -> Option<String> {
    let name =
        gh_repo.and_then(|r| r.split('/').nth(1)).map(|s| s.to_string()).or_else(
            || match source {
                "core-script" | "core" => Some("core".into()),
                "state" => Some("state".into()),
                s if s.starts_with("plugin-") => Some(s.to_string()),
                s => Some(s.to_string()),
            },
        )?;
    Some(if name.starts_with("plugin-") { format!("plugins/{name}") } else { name })
}

fn local_paths(source: &str, suite: &str, format: &str, gh_repo: Option<&str>) -> Vec<String> {
    let Some(dir) = repo_dir_for(source, gh_repo) else { return vec![] };
    match (format, suite) {
        (_, "conformance") => vec!["state/conformance/out/paged-results-*.json".into()],
        (_, "fidelity") => vec![
            "corpus/out/reports/*/gate.json".into(),
            "core/corpus/generated/out/*/gate.json".into(),
        ],
        ("playwright", _) => vec![format!("{dir}/**/test-results/results.json")],
        ("junit", s) if s.contains("nextest") => vec![format!("{dir}/target/nextest/ci/junit.xml")],
        ("junit", _) => vec![format!("{dir}/**/junit.xml")],
        _ => vec![],
    }
}

// ── YAML helpers ────────────────────────────────────────────────────────────

fn load_chapters(path: &Path) -> anyhow::Result<Vec<Chapter>> {
    let text = std::fs::read_to_string(path)?;
    let v: Y = serde_yaml_ng::from_str(&text)?;
    let Y::Sequence(items) = v else { anyhow::bail!("{} is not a list", path.display()) };
    // Chapter header comments are the note; recover them from the raw text by id.
    Ok(items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| {
            let m = it.as_mapping()?;
            Some(Chapter {
                id: str_of(m, "id")?,
                title: str_of(m, "title").unwrap_or_default(),
                group: str_of(m, "group").unwrap_or_else(|| "format".into()),
                order: i + 1,
                note: None,
            })
        })
        .collect())
}

fn load_list(path: &Path) -> anyhow::Result<Vec<Mapping>> {
    let text = std::fs::read_to_string(path)?;
    let v: Y = serde_yaml_ng::from_str(&text)?;
    match v {
        Y::Sequence(items) => {
            Ok(items.into_iter().filter_map(|i| i.as_mapping().cloned()).collect())
        }
        Y::Null => Ok(vec![]),
        _ => anyhow::bail!("{} is not a list", path.display()),
    }
}

fn load_mapping_list(path: &Path, key: &str) -> anyhow::Result<Vec<Mapping>> {
    let text = std::fs::read_to_string(path)?;
    let v: Y = serde_yaml_ng::from_str(&text)?;
    match v.get(Y::String(key.into())) {
        Some(Y::Sequence(items)) => {
            Ok(items.iter().filter_map(|i| i.as_mapping().cloned()).collect())
        }
        _ => Ok(vec![]),
    }
}

fn load_features(dir: &Path, report: &mut Report) -> anyhow::Result<Vec<(Mapping, String)>> {
    let mut out = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    let mut seen = std::collections::HashSet::new();
    for e in entries {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("yaml") {
            continue;
        }
        let name = format!("registry/features/{}", p.file_name().unwrap().to_string_lossy());
        for row in load_list(&p)? {
            match str_of(&row, "id") {
                Some(id) if seen.insert(id.clone()) => out.push((row, name.clone())),
                Some(id) => report.warnings.push(format!("duplicate id {id} in {name}")),
                None => report.warnings.push(format!("row without id in {name}")),
            }
        }
    }
    Ok(out)
}

fn str_of(m: &Mapping, key: &str) -> Option<String> {
    match m.get(Y::String(key.into()))? {
        Y::String(s) => Some(s.clone()),
        Y::Number(n) => Some(n.to_string()),
        Y::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn int_of(m: &Mapping, key: &str) -> Option<i64> {
    m.get(Y::String(key.into()))?.as_i64()
}

fn list_of(v: &Y) -> Vec<String> {
    match v {
        Y::Sequence(s) => s.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect(),
        Y::String(s) => vec![s.clone()],
        _ => vec![],
    }
}

fn toml_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
}

fn yaml_str(s: &str) -> String {
    let v = serde_yaml_ng::to_string(&Y::String(s.to_string())).unwrap_or_default();
    v.trim_end().to_string()
}
