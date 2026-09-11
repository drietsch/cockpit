//! Provenance for imported evidence: declared by the producer, read from a pull sidecar, or
//! inferred — and always marked with which.

use super::ImportContext;
use crate::config::EvidenceFormat;
use crate::hash::ContentHash;
use crate::ids::{CommitKey, RepoId, Sha, SourceId};
use crate::model::{CommitOrigin, CommitRef, Provenance, ToolInfo};
use crate::paths::RelPath;
use crate::time::{Origin, Stamped, parse_timestamp};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// `cockpit.provenance.v1` — written by `cockpit pull` next to downloaded artifacts.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Sidecar {
    pub schema: String,
    pub source: Option<String>,
    pub repo: Option<String>,
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub run: Option<SidecarRun>,
    pub generated_at: Option<String>,
    pub artifact: Option<serde_json::Value>,
    pub fetched_at: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SidecarRun {
    pub id: Option<serde_json::Value>,
    pub url: Option<String>,
    pub attempt: Option<u32>,
    pub workflow: Option<String>,
    pub conclusion: Option<String>,
    pub event: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl SidecarRun {
    pub fn id_string(&self) -> Option<String> {
        match &self.id {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            _ => None,
        }
    }
}

/// Sidecars indexed by directory; lookup walks up from a file to the nearest one.
#[derive(Debug, Default, Clone)]
pub struct SidecarIndex {
    by_dir: HashMap<PathBuf, Sidecar>,
}

impl SidecarIndex {
    pub fn load(evidence_dir: &Path) -> SidecarIndex {
        let mut idx = SidecarIndex::default();
        if !evidence_dir.is_dir() {
            return idx;
        }
        let mut builder = ignore::WalkBuilder::new(evidence_dir);
        builder.standard_filters(false).hidden(false).max_depth(Some(6));
        for e in builder.build().flatten() {
            if e.file_name() == "provenance.json" && e.path().is_file() {
                if let Ok(text) = std::fs::read_to_string(e.path()) {
                    if let Ok(sc) = serde_json::from_str::<Sidecar>(&text) {
                        if let Some(dir) = e.path().parent() {
                            idx.by_dir.insert(dir.to_path_buf(), sc);
                        }
                    }
                }
            }
        }
        idx
    }

    pub fn lookup(&self, abs: &Path) -> Option<&Sidecar> {
        let mut cur = abs.parent();
        while let Some(d) = cur {
            if let Some(s) = self.by_dir.get(d) {
                return Some(s);
            }
            cur = d.parent();
        }
        None
    }
}

/// Per-repo commit times, for inferring which commit a timestamp belongs to.
#[derive(Debug, Default, Clone)]
pub struct CommitLocator {
    /// repo → (committed_at, sha) sorted ascending by time
    times: HashMap<RepoId, Vec<(Timestamp, Sha)>>,
    heads: HashMap<RepoId, (Sha, Timestamp)>,
    known: HashMap<RepoId, std::collections::HashSet<Sha>>,
}

impl CommitLocator {
    pub fn insert(&mut self, repo: RepoId, sha: Sha, committed_at: Timestamp) {
        self.times.entry(repo.clone()).or_default().push((committed_at, sha.clone()));
        self.known.entry(repo).or_default().insert(sha);
    }

    pub fn set_head(&mut self, repo: RepoId, sha: Sha, committed_at: Timestamp) {
        self.heads.insert(repo, (sha, committed_at));
    }

    pub fn finish(&mut self) {
        for v in self.times.values_mut() {
            v.sort();
        }
    }

    pub fn is_known(&self, repo: &RepoId, sha: &Sha) -> bool {
        self.known.get(repo).map(|s| s.contains(sha)).unwrap_or(false)
    }

    /// Resolve a possibly abbreviated sha within a repo.
    pub fn resolve_prefix(&self, repo: &RepoId, prefix: &str) -> Option<Sha> {
        let p = prefix.to_ascii_lowercase();
        let set = self.known.get(repo)?;
        if set.contains(&Sha::new(p.clone())) {
            return Some(Sha::new(p));
        }
        let mut hits = set.iter().filter(|s| s.as_str().starts_with(&p));
        let first = hits.next()?.clone();
        if hits.next().is_some() {
            return None;
        }
        Some(first)
    }

    /// The newest commit at or before `at`; `InferredHead` when `at` is after HEAD.
    pub fn commit_at(&self, repo: &RepoId, at: Timestamp) -> Option<(Sha, CommitOrigin)> {
        if let Some((head, head_at)) = self.heads.get(repo) {
            if at >= *head_at {
                return Some((head.clone(), CommitOrigin::InferredHead));
            }
        }
        let v = self.times.get(repo)?;
        let idx = v.partition_point(|(t, _)| *t <= at);
        if idx == 0 {
            return None;
        }
        Some((v[idx - 1].1.clone(), CommitOrigin::InferredByTimestamp))
    }
}

/// What the document itself declared.
#[derive(Debug, Default, Clone)]
pub struct Declared {
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub timestamp: Option<Timestamp>,
    pub tool: Option<ToolInfo>,
    pub run_id: Option<String>,
    pub run_url: Option<String>,
    pub dirty: Option<bool>,
    pub repo: Option<RepoId>,
}

pub struct Built {
    pub provenance: Provenance,
    pub timestamp: Stamped<Timestamp>,
    pub notes: Vec<String>,
}

/// Precedence per field: declared → sidecar → inferred; every choice recorded.
#[allow(clippy::too_many_arguments)]
pub fn build(
    ctx: &ImportContext,
    format: EvidenceFormat,
    version: u32,
    input_rel: &RelPath,
    input_abs: &Path,
    content_hash: ContentHash,
    mtime: Timestamp,
    declared: Declared,
) -> Built {
    let mut notes = Vec::new();
    let sidecar = ctx.sidecars.lookup(input_abs);
    let source_id = ctx.source_id.clone().unwrap_or_else(|| SourceId::new("auto"));
    let repo: RepoId = declared
        .repo
        .as_ref()
        .and_then(|r| ctx.repos.resolve(r.as_str()))
        .or_else(|| ctx.source.and_then(|s| s.repo.as_deref()).and_then(|r| ctx.repos.resolve(r)))
        .or_else(|| sidecar.and_then(|s| s.repo.as_deref()).and_then(|r| ctx.repos.resolve(r)))
        .or_else(|| ctx.repos.repo_for(input_rel).cloned())
        .or_else(|| ctx.project.repos.first().map(|r| r.id.clone()))
        .unwrap_or_else(|| RepoId::new(ctx.project.id.clone()));

    // Timestamp.
    let timestamp: Stamped<Timestamp> = if let Some(t) = declared.timestamp {
        Stamped::new(t, Origin::Declared)
    } else if let Some(t) =
        sidecar.and_then(|s| s.generated_at.as_deref()).and_then(parse_timestamp)
    {
        Stamped::new(t, Origin::Sidecar)
    } else {
        Stamped::new(mtime, Origin::FileMtime)
    };

    // Commit.
    let commit: Option<CommitRef> = if let Some(c) =
        declared.commit.as_deref().or_else(|| sidecar.and_then(|s| s.commit.as_deref()))
    {
        let origin =
            if declared.commit.is_some() { CommitOrigin::Declared } else { CommitOrigin::Sidecar };
        match ctx.commits.resolve_prefix(&repo, c) {
            Some(sha) => Some(CommitRef {
                key: CommitKey { repo: repo.clone(), sha },
                origin,
                resolved: true,
            }),
            None => {
                notes.push(format!("commit {c} is not in the local history of {repo}"));
                Some(CommitRef {
                    key: CommitKey { repo: repo.clone(), sha: Sha::new(c.to_ascii_lowercase()) },
                    origin,
                    resolved: false,
                })
            }
        }
    } else {
        ctx.commits.commit_at(&repo, timestamp.value).map(|(sha, origin)| CommitRef {
            key: CommitKey { repo: repo.clone(), sha },
            origin,
            resolved: true,
        })
    };

    let branch = declared.branch.clone().or_else(|| sidecar.and_then(|s| s.branch.clone()));
    let run_id = declared
        .run_id
        .clone()
        .or_else(|| sidecar.and_then(|s| s.run.as_ref()).and_then(|r| r.id_string()));
    let run_url = declared
        .run_url
        .clone()
        .or_else(|| sidecar.and_then(|s| s.run.as_ref()).and_then(|r| r.url.clone()));

    Built {
        provenance: Provenance {
            source: source_id,
            source_path: input_rel.clone(),
            format,
            importer_version: version,
            content_hash,
            commit,
            branch,
            timestamp: Some(timestamp),
            tool: declared.tool.clone(),
            environment: None,
            run_id,
            run_url,
            dirty: declared.dirty,
            imported_at: ctx.now,
        },
        timestamp,
        notes,
    }
}
