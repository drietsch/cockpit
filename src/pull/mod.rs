//! `cockpit pull`: fetch the newest CI artifact for every GitHub-backed evidence source into
//! `.cockpit/evidence/<source>/<artifact>/`, with a `cockpit.provenance.v1` sidecar carrying
//! the run's real completion time, commit and ids. Runs are considered regardless of their
//! conclusion — a red run with a JUnit file is evidence, not noise.

pub mod github;

use crate::cli::output::Format;
use crate::config::{EvidenceSource, Located};
use crate::time::now;
use github::{ArtifactInfo, GithubApi, RunInfo};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Pulled { run_id: u64, artifact_id: u64, files: usize, bytes: u64 },
    UpToDate { run_id: u64 },
    Skipped(String),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct SourceReport {
    pub source: String,
    pub github: String,
    pub outcome: Outcome,
    pub commit: Option<String>,
    pub finished_at: Option<String>,
    pub url: Option<String>,
}

pub struct Options {
    pub dry_run: bool,
    pub force: bool,
    pub only: Vec<String>,
}

/// Where a source's artifact lands: `<evidence dir>/<source id with '/' → '__'>/<artifact>/`.
pub fn target_dir(evidence_dir: &Path, source_id: &str, artifact: &str) -> PathBuf {
    evidence_dir.join(source_id.replace('/', "__")).join(artifact)
}

pub fn run(located: &Located, opts: &Options, fmt: &Format) -> anyhow::Result<Vec<SourceReport>> {
    let sources: Vec<EvidenceSource> = located
        .evidence_sources()
        .into_iter()
        .filter(|s| s.github.is_some() && s.enabled)
        .filter(|s| {
            opts.only.is_empty()
                || opts.only.iter().any(|g| {
                    globset::Glob::new(g)
                        .ok()
                        .map(|g| g.compile_matcher().is_match(s.id.as_deref().unwrap_or("")))
                        .unwrap_or(false)
                })
        })
        .collect();
    let evidence_dir = located.evidence_dir();
    let api = github::detect();
    let mut reports = Vec::new();
    if sources.is_empty() {
        return Ok(reports);
    }
    let api = match api {
        Some(a) => a,
        None => {
            for s in &sources {
                let gh = s.github.as_ref().unwrap();
                reports.push(SourceReport {
                    source: s.id.clone().unwrap_or_default(),
                    github: format!("{} · {} · {}", gh.repo, gh.workflow, gh.artifact),
                    outcome: Outcome::Skipped(
                        "no GitHub credentials (gh auth login, or GITHUB_TOKEN)".into(),
                    ),
                    commit: None,
                    finished_at: None,
                    url: None,
                });
            }
            return Ok(reports);
        }
    };
    tracing::info!("pulling with {}", api.describe());
    // Sources are independent: a few at a time keeps the API polite and the wall clock short.
    let api: &dyn GithubApi = &*api;
    let mut slots: Vec<Option<SourceReport>> = vec![None; sources.len()];
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done = std::sync::Mutex::new(Vec::<usize>::new());
    std::thread::scope(|scope| {
        let workers = sources.len().min(4);
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let next = &next;
                let sources = &sources;
                let evidence_dir = &evidence_dir;
                let done = &done;
                scope.spawn(move || {
                    let mut mine: Vec<(usize, SourceReport)> = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        if i >= sources.len() {
                            break;
                        }
                        let s = &sources[i];
                        let gh = s.github.as_ref().unwrap();
                        let id = s.id.clone().unwrap_or_default();
                        let label = format!("{} · {} · {}", gh.repo, gh.workflow, gh.artifact);
                        let rep = match pull_one(api, s, evidence_dir, opts) {
                            Ok(rep) => rep,
                            Err(e) => SourceReport {
                                source: id.clone(),
                                github: label.clone(),
                                outcome: Outcome::Failed(e.to_string()),
                                commit: None,
                                finished_at: None,
                                url: None,
                            },
                        };
                        done.lock().unwrap().push(i);
                        mine.push((i, rep));
                    }
                    mine
                })
            })
            .collect();
        for h in handles {
            for (i, rep) in h.join().unwrap_or_default() {
                slots[i] = Some(rep);
            }
        }
    });
    for rep in slots.into_iter().flatten() {
        if !fmt.json {
            eprintln!("{} {} … {}", fmt.dim("pull"), rep.source, describe(&rep, fmt));
        }
        reports.push(rep);
    }
    Ok(reports)
}

fn describe(r: &SourceReport, fmt: &Format) -> String {
    match &r.outcome {
        Outcome::Pulled { run_id, files, bytes, .. } => fmt.paint(
            "32",
            &format!(
                "pulled (new) run {run_id} · {files} files · {}",
                crate::server::view::bytes(*bytes)
            ),
        ),
        Outcome::UpToDate { run_id } => fmt.dim(&format!("= up to date (run {run_id})")),
        Outcome::Skipped(why) => fmt.dim(&format!("· skipped — {why}")),
        Outcome::Failed(why) => fmt.paint("31", &format!("✕ {why}")),
    }
}

fn pull_one(
    api: &dyn GithubApi,
    s: &EvidenceSource,
    evidence_dir: &Path,
    opts: &Options,
) -> anyhow::Result<SourceReport> {
    let gh = s.github.as_ref().unwrap();
    let id = s.id.clone().unwrap_or_default();
    let label = format!("{} · {} · {}", gh.repo, gh.workflow, gh.artifact);
    let branch = gh.branch.clone().unwrap_or_else(|| "main".into());
    let inspect = gh.runs_to_inspect.unwrap_or(20).clamp(1, 100);
    let mut report = SourceReport {
        source: id.clone(),
        github: label,
        outcome: Outcome::Skipped(String::new()),
        commit: None,
        finished_at: None,
        url: None,
    };
    let runs = api.list_runs(&gh.repo, &gh.workflow, &branch, inspect)?;
    if runs.is_empty() {
        report.outcome =
            Outcome::Skipped(format!("no completed runs of {} on {branch}", gh.workflow));
        return Ok(report);
    }
    // Newest run that actually carries the artifact (any conclusion).
    let mut chosen: Option<(RunInfo, ArtifactInfo)> = None;
    for run in &runs {
        let arts = api.list_artifacts(&gh.repo, run.id)?;
        if let Some(a) = arts.into_iter().find(|a| a.name == gh.artifact && !a.expired) {
            chosen = Some((run.clone(), a));
            break;
        }
    }
    let Some((run, art)) = chosen else {
        report.outcome = Outcome::Skipped(format!(
            "artifact {} on none of the last {} runs",
            gh.artifact,
            runs.len()
        ));
        return Ok(report);
    };
    report.commit = Some(run.head_sha.clone());
    report.finished_at = Some(run.updated_at.clone());
    report.url = Some(run.html_url.clone());
    let dir = target_dir(evidence_dir, &id, &gh.artifact);
    let sidecar_path = dir.join("provenance.json");
    if !opts.force {
        if let Ok(text) = std::fs::read_to_string(&sidecar_path) {
            if let Ok(sc) = serde_json::from_str::<serde_json::Value>(&text) {
                let same_run = sc.pointer("/run/id").and_then(|v| v.as_u64()) == Some(run.id);
                let same_art = sc.pointer("/artifact/id").and_then(|v| v.as_u64()) == Some(art.id);
                if same_run && same_art {
                    report.outcome = Outcome::UpToDate { run_id: run.id };
                    return Ok(report);
                }
            }
        }
    }
    if opts.dry_run {
        report.outcome = Outcome::Pulled {
            run_id: run.id,
            artifact_id: art.id,
            files: 0,
            bytes: art.size_in_bytes,
        };
        return Ok(report);
    }
    let max = crate::config::Config::default().pull.max_artifact_bytes;
    if art.size_in_bytes > max {
        report.outcome = Outcome::Failed(format!(
            "artifact is {} bytes, above pull.max_artifact_bytes",
            art.size_in_bytes
        ));
        return Ok(report);
    }
    // Download → extract into a temp dir → atomic swap.
    let tmp_root = evidence_dir.join(".tmp");
    std::fs::create_dir_all(&tmp_root)?;
    let zip_path = tmp_root.join(format!("{}-{}.zip", id.replace('/', "__"), art.id));
    api.download_artifact(&gh.repo, &art, &zip_path)?;
    let extract_dir = tmp_root.join(format!("{}-{}", id.replace('/', "__"), art.id));
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir)?;
    let files = extract_zip(&zip_path, &extract_dir)?;
    let _ = std::fs::remove_file(&zip_path);
    let sidecar = serde_json::json!({
        "schema": "cockpit.provenance.v1",
        "source": id,
        "repo": s.repo,
        "commit": run.head_sha,
        "branch": run.head_branch.clone().unwrap_or(branch),
        "run": { "id": run.id, "url": run.html_url, "attempt": run.run_attempt, "workflow": gh.workflow, "conclusion": run.conclusion, "event": run.event, "started_at": run.run_started_at, "finished_at": run.updated_at },
        "generated_at": art.created_at.clone().unwrap_or(run.updated_at.clone()),
        "artifact": { "id": art.id, "name": art.name, "size": art.size_in_bytes, "expired": art.expired, "files": files },
        "fetched_at": now().to_string(),
        "fetched_by": format!("cockpit {}", crate::VERSION),
    });
    std::fs::write(extract_dir.join("provenance.json"), serde_json::to_string_pretty(&sidecar)?)?;
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let old = tmp_root.join(format!("{}-{}.old", id.replace('/', "__"), art.id));
    let _ = std::fs::remove_dir_all(&old);
    if dir.exists() {
        std::fs::rename(&dir, &old)?;
    }
    std::fs::rename(&extract_dir, &dir)?;
    let _ = std::fs::remove_dir_all(&old);
    report.outcome = Outcome::Pulled {
        run_id: run.id,
        artifact_id: art.id,
        files: files.len(),
        bytes: art.size_in_bytes,
    };
    Ok(report)
}

/// Extract with zip-slip protection: no absolute paths, no `..`, no symlinks, 10k files.
fn extract_zip(zip_path: &Path, dest: &Path) -> anyhow::Result<Vec<String>> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut files = Vec::new();
    if archive.len() > 10_000 {
        anyhow::bail!("artifact has {} entries (limit 10000)", archive.len());
    }
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            anyhow::bail!("refusing entry with an unsafe path: {}", entry.name());
        };
        if entry.is_dir() {
            std::fs::create_dir_all(dest.join(&rel))?;
            continue;
        }
        if let Some(mode) = entry.unix_mode() {
            if mode & 0o170000 == 0o120000 {
                anyhow::bail!("refusing symlink entry {}", entry.name());
            }
        }
        let out = dest.join(&rel);
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut f = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut f)?;
        files.push(rel.to_string_lossy().replace('\\', "/"));
    }
    Ok(files)
}
