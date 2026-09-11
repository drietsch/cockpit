//! The scan pipeline: discover repos, sync git, walk documents and evidence, import what
//! changed, persist, then rebuild the in-memory state.

pub mod classify;
pub mod walk;
pub mod watch;

use crate::config::{Located, RepoSpec};
use crate::git::{GitBackend, GitError};
use crate::hash::ContentHash;
use crate::ids::{CommitKey, RepoId, Sha};
use crate::import::{
    ImportContext, ImportInput, ImportedEntity, Importer, ImporterKind, RepoIndex,
};
use crate::model::*;
use crate::paths::{RelPath, relativize};
use crate::store::Store;
use crate::store::write::ScanFileRecord;
use crate::time::{millis, now as time_now};
use jiff::Timestamp;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error(transparent)]
    Store(#[from] crate::store::StoreError),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ScanError>;

#[derive(Debug, Clone, Default)]
pub struct ScanCounts {
    pub repos: usize,
    pub commits: usize,
    pub new_commits: usize,
    pub features: usize,
    pub adrs: usize,
    pub tests: usize,
    pub runs: usize,
    pub benchmarks: usize,
    pub visuals: usize,
    pub files_seen: usize,
    pub files_imported: usize,
    pub warnings: usize,
}

#[derive(Debug, Clone)]
pub enum ScanProgress {
    Started { full: bool },
    Phase(&'static str),
    Counts(ScanCounts),
    Finished(ScanReport),
}

#[derive(Debug, Clone)]
pub struct ScanReport {
    pub started_at: Timestamp,
    pub duration_ms: u64,
    pub counts: ScanCounts,
    pub warnings: Vec<ImportWarning>,
    pub repo_notes: Vec<(RepoId, String)>,
}

pub type ProgressFn<'a> = dyn Fn(ScanProgress) + Send + Sync + 'a;

pub struct Scanner<'a> {
    pub located: &'a Located,
    pub git: &'a dyn GitBackend,
    pub progress: Option<&'a ProgressFn<'a>>,
}

impl<'a> Scanner<'a> {
    pub fn new(located: &'a Located, git: &'a dyn GitBackend) -> Self {
        Scanner { located, git, progress: None }
    }

    pub fn with_progress(mut self, f: &'a ProgressFn<'a>) -> Self {
        self.progress = Some(f);
        self
    }

    fn emit(&self, p: ScanProgress) {
        if let Some(f) = self.progress {
            f(p);
        }
    }

    /// Full scan: every source is (re)considered; unchanged files are skipped by mtime/hash.
    pub fn full(&self, store: &mut Store) -> Result<ScanReport> {
        let started = Instant::now();
        let now = time_now();
        self.emit(ScanProgress::Started { full: true });
        let mut counts = ScanCounts::default();
        let mut warnings: Vec<ImportWarning> = Vec::new();
        let mut repo_notes = Vec::new();

        // 1. Repositories: read git in parallel (independent subprocesses), then persist.
        self.emit(ScanProgress::Phase("git"));
        let mut repos = discover_repos(self.located);
        let plans: Vec<RepoSyncPlan> = {
            let store_ref: &Store = store;
            let git = self.git;
            let cfg = &self.located.config;
            let root = &self.located.root;
            let default_branch = cfg.project.primary_branch.clone();
            std::thread::scope(|scope| {
                let handles: Vec<_> = repos
                    .iter()
                    .map(|repo| {
                        let abs = repo.path.to_abs(root);
                        let prev_head =
                            store_ref.meta(&format!("repo_head:{}", repo.id)).ok().flatten();
                        let known_count =
                            store_ref.known_shas(&repo.id).map(|v| v.len()).unwrap_or(0);
                        let repo_id = repo.id.clone();
                        let primary =
                            repo.primary_branch.clone().unwrap_or_else(|| default_branch.clone());
                        scope.spawn(move || {
                            read_repo(
                                git,
                                &abs,
                                &repo_id,
                                prev_head,
                                known_count,
                                cfg.git.max_commits,
                                cfg.git.dirty_check,
                                &primary,
                            )
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| {
                        h.join().unwrap_or_else(|_| RepoSyncPlan::failed("git thread panicked"))
                    })
                    .collect()
            })
        };
        store.batch_begin()?;
        for (repo, plan) in repos.iter_mut().zip(plans) {
            match plan.error {
                Some(e) => {
                    let note = format!("git sync failed: {e}");
                    tracing::warn!(repo = %repo.id, "{note}");
                    repo.note = Some(note);
                }
                None => {
                    if plan.rewritten {
                        store.delete_commits_of_repo(&repo.id)?;
                    }
                    let prefix = repo.path.clone();
                    for rc in &plan.commits {
                        store.upsert_commit(&to_commit(&repo.id, &prefix, rc))?;
                        counts.new_commits += 1;
                    }
                    if let Some(h) = &plan.head {
                        store.set_meta(&format!("repo_head:{}", repo.id), &h.sha)?;
                        repo.head = Some(RepoHead {
                            sha: Sha::new(h.sha.clone()),
                            branch: h.branch.clone(),
                            dirty: h.dirty_files > 0,
                            dirty_files: h.dirty_files,
                            observed_at: now,
                        });
                    }
                    repo.commit_count = plan.commit_count;
                    repo.note = plan.note.clone();
                    counts.commits += store.known_shas(&repo.id)?.len();
                }
            }
            if let Some(n) = &repo.note {
                repo_notes.push((repo.id.clone(), n.clone()));
            }
            store.upsert_repo(repo, now)?;
        }
        store.batch_commit()?;
        counts.repos = repos.iter().filter(|r| r.head.is_some()).count();
        self.emit(ScanProgress::Counts(counts.clone()));

        let project = self.project(&repos);
        let repo_index = RepoIndex::new(&repos);

        // 2. Documents: features first (so ADRs and evidence can resolve ids), then ADRs.
        self.emit(ScanProgress::Phase("documents"));
        store.batch_begin()?;
        let known = store.scan_files()?;
        let mut seen: HashSet<RelPath> = HashSet::new();
        let empty_ids: HashSet<crate::ids::FeatureId> = HashSet::new();
        let feature_files = walk::feature_files(self.located);
        let mut feature_ids: HashSet<crate::ids::FeatureId> = HashSet::new();
        {
            let ctx = ImportContext {
                located: self.located,
                project: &project,
                repos: &repo_index,
                feature_ids: &empty_ids,
                source: None,
                source_id: None,
                now,
            };
            let imp = crate::import::feature_md::FeatureMdImporter;
            for abs in &feature_files {
                if let Some(w) = self.import_document(
                    store,
                    &ctx,
                    &imp,
                    abs,
                    &known,
                    &mut seen,
                    &mut counts,
                    now,
                )? {
                    warnings.extend(w);
                }
            }
            for f in store.features()? {
                feature_ids.insert(f.id);
            }
            counts.features = feature_ids.len();
            self.emit(ScanProgress::Counts(counts.clone()));
        }
        {
            let ctx = ImportContext {
                located: self.located,
                project: &project,
                repos: &repo_index,
                feature_ids: &feature_ids,
                source: None,
                source_id: None,
                now,
            };
            let imp = crate::import::adr_md::AdrMdImporter;
            for abs in walk::adr_files(self.located) {
                if let Some(w) = self.import_document(
                    store,
                    &ctx,
                    &imp,
                    &abs,
                    &known,
                    &mut seen,
                    &mut counts,
                    now,
                )? {
                    warnings.extend(w);
                }
            }
            counts.adrs = store.count("adrs")? as usize;
            self.emit(ScanProgress::Counts(counts.clone()));
        }

        // 3. Files that vanished since the last scan.
        for (path, info) in &known {
            if !seen.contains(path) && (info.importer == "feature-md" || info.importer == "adr-md")
            {
                store.delete_by_source_path(path)?;
            }
        }
        counts.features = store.count("features")? as usize;
        counts.adrs = store.count("adrs")? as usize;
        store.batch_commit()?;

        // Carry warnings of unchanged files too, so the banner is complete.
        let all_warnings = store.warnings()?;
        counts.warnings = all_warnings.len();
        store.set_meta_timestamp("last_scan_at", now)?;
        store.set_meta("last_scan_full", "1")?;

        let report = ScanReport {
            started_at: now,
            duration_ms: started.elapsed().as_millis() as u64,
            counts,
            warnings: all_warnings,
            repo_notes,
        };
        self.emit(ScanProgress::Finished(report.clone()));
        Ok(report)
    }

    #[allow(clippy::too_many_arguments)]
    fn import_document(
        &self,
        store: &mut Store,
        ctx: &ImportContext,
        importer: &dyn Importer,
        abs: &Path,
        known: &std::collections::HashMap<RelPath, crate::store::read::ScanFileInfo>,
        seen: &mut HashSet<RelPath>,
        counts: &mut ScanCounts,
        now: Timestamp,
    ) -> Result<Option<Vec<ImportWarning>>> {
        let Some(rel) = relativize(&self.located.root, abs) else { return Ok(None) };
        counts.files_seen += 1;
        seen.insert(rel.clone());
        let meta = std::fs::metadata(abs)?;
        let mtime = meta.modified().ok().and_then(|t| Timestamp::try_from(t).ok()).unwrap_or(now);
        let size = meta.len();
        if let Some(prev) = known.get(&rel) {
            if prev.importer == importer.kind().as_str()
                && prev.importer_version == importer.version()
                && prev.size == size
                && millis(prev.mtime) == millis(mtime)
            {
                return Ok(None);
            }
        }
        let bytes = std::fs::read(abs)?;
        let content_hash = ContentHash::of(&bytes);
        if let Some(prev) = known.get(&rel) {
            if prev.content_hash == content_hash.hex()
                && prev.importer_version == importer.version()
            {
                // Touched but unchanged: refresh the mtime record only.
                store.upsert_scan_file(
                    &ScanFileRecord {
                        path: rel.clone(),
                        mtime,
                        size,
                        content_hash,
                        importer: importer.kind().as_str(),
                        importer_version: importer.version(),
                        source_id: None,
                        status: "ok".into(),
                        warnings: vec![],
                    },
                    now,
                )?;
                return Ok(None);
            }
        }
        let input = ImportInput {
            abs: abs.to_path_buf(),
            rel: rel.clone(),
            bytes,
            content_hash,
            mtime,
            size,
        };
        let out = importer.import(ctx, &input);
        counts.files_imported += 1;
        for entity in &out.entities {
            match entity {
                ImportedEntity::Feature(f) => store.upsert_feature(f, &content_hash, now)?,
                ImportedEntity::Adr(a) => store.upsert_adr(a, &content_hash, now)?,
                _ => {}
            }
        }
        let status = if out.warnings.iter().any(|w| w.fatal) {
            "error"
        } else if out.warnings.is_empty() {
            "ok"
        } else {
            "warning"
        };
        store.upsert_scan_file(
            &ScanFileRecord {
                path: rel.clone(),
                mtime,
                size,
                content_hash,
                importer: importer.kind().as_str(),
                importer_version: importer.version(),
                source_id: None,
                status: status.into(),
                warnings: out.warnings.clone(),
            },
            now,
        )?;
        store.replace_warnings(&rel, &out.warnings, now)?;
        Ok(Some(out.warnings))
    }

    pub fn project(&self, repos: &[Repo]) -> Project {
        Project {
            id: self.located.project_id(),
            name: self.located.project_name(),
            root: self.located.root.clone(),
            repos: repos.to_vec(),
            components: self
                .located
                .config
                .components
                .iter()
                .enumerate()
                .map(|(i, c)| Component {
                    id: crate::ids::ComponentId::new(&c.id),
                    title: c.title.clone().unwrap_or_else(|| c.id.clone()),
                    group: c
                        .group
                        .clone()
                        .or_else(|| c.id.split('.').next().map(|s| s.to_string())),
                    order: c.order.unwrap_or(i as u32 + 1),
                })
                .collect(),
            primary_branch: self.located.config.project.primary_branch.clone(),
        }
    }
}

/// What one repository's git read produced; applied to the store afterwards.
struct RepoSyncPlan {
    head: Option<crate::git::HeadInfo>,
    commits: Vec<crate::git::RawCommit>,
    commit_count: u64,
    rewritten: bool,
    note: Option<String>,
    error: Option<String>,
}

impl RepoSyncPlan {
    fn failed(msg: &str) -> Self {
        RepoSyncPlan {
            head: None,
            commits: vec![],
            commit_count: 0,
            rewritten: false,
            note: None,
            error: Some(msg.into()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn read_repo(
    git: &dyn GitBackend,
    abs: &Path,
    _repo_id: &RepoId,
    prev_head: Option<String>,
    known_count: usize,
    max: usize,
    dirty_check: bool,
    primary_branch: &str,
) -> RepoSyncPlan {
    let head = match git.head(abs, dirty_check) {
        Ok(Some(h)) => h,
        Ok(None) => {
            return RepoSyncPlan {
                head: None,
                commits: vec![],
                commit_count: 0,
                rewritten: false,
                note: Some("0 commits, skipped".into()),
                error: None,
            };
        }
        Err(e) => return RepoSyncPlan::failed(&e.to_string()),
    };
    let commit_count = git.commit_count(abs).unwrap_or(0);
    let mut rewritten = false;
    let commits = match prev_head.filter(|_| known_count > 0) {
        None => git.log(abs, None, max),
        Some(prev) if prev == head.sha => Ok(vec![]),
        Some(prev) if git.is_ancestor(abs, &prev, &head.sha).unwrap_or(false) => {
            git.log(abs, Some(&format!("{prev}..{}", head.sha)), max)
        }
        Some(_) => {
            rewritten = true;
            git.log(abs, None, max)
        }
    };
    let commits = match commits {
        Ok(c) => c,
        Err(e) => return RepoSyncPlan::failed(&e.to_string()),
    };
    let note = match &head.branch {
        Some(b) if b != primary_branch => Some(format!("on branch {b}, not {primary_branch}")),
        _ => None,
    };
    RepoSyncPlan { head: Some(head), commits, commit_count, rewritten, note, error: None }
}

fn to_commit(repo: &RepoId, prefix: &RelPath, rc: &crate::git::RawCommit) -> Commit {
    let mut trailers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, v) in &rc.trailers {
        trailers.entry(k.clone()).or_default().push(v.clone());
    }
    Commit {
        key: CommitKey { repo: repo.clone(), sha: Sha::new(rc.sha.clone()) },
        parents: rc.parents.iter().map(|p| Sha::new(p.clone())).collect(),
        author: rc.author.clone(),
        author_email: rc.author_email.clone(),
        authored_at: rc.authored_at,
        committed_at: rc.committed_at,
        subject: rc.subject.clone(),
        body: rc.body.clone(),
        refs: rc.refs.clone(),
        files: rc
            .files
            .iter()
            .map(|f| ChangedFile {
                path: prefix.join(&f.path),
                repo_path: f.path.clone(),
                kind: if f.binary {
                    ChangeKind::Binary
                } else if f.rename_from.is_some() {
                    ChangeKind::Renamed
                } else if f.deleted.is_none() && f.added.is_some() {
                    ChangeKind::Added
                } else {
                    ChangeKind::Modified
                },
                added: f.added,
                deleted: f.deleted,
                rename_from: f.rename_from.clone(),
            })
            .collect(),
        trailers,
    }
}

/// Repositories of the project: configured, else the root itself when it is a repo, else
/// every directory (depth ≤ 2) containing `.git`.
pub fn discover_repos(located: &Located) -> Vec<Repo> {
    let root = &located.root;
    let mk = |spec: &RepoSpec| {
        let path = RelPath::new(&spec.path);
        let id = spec.id.clone().unwrap_or_else(|| {
            if path.is_root() { located.project_id() } else { path.file_name().to_string() }
        });
        Repo {
            id: RepoId::new(id),
            path,
            primary_branch: spec.primary_branch.clone(),
            head: None,
            commit_count: 0,
            note: None,
        }
    };
    if !located.config.repos.is_empty() {
        return located.config.repos.iter().map(mk).collect();
    }
    if root.join(".git").exists() {
        return vec![mk(&RepoSpec { id: None, path: ".".into(), primary_branch: None })];
    }
    let mut found: Vec<Repo> = Vec::new();
    let mut dirs: Vec<(std::path::PathBuf, usize)> = vec![(root.clone(), 0)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            if p.join(".git").exists() {
                let rel = relativize(root, &p).unwrap_or_else(|| RelPath::new(&name));
                found.push(mk(&RepoSpec {
                    id: None,
                    path: rel.as_str().to_string(),
                    primary_branch: None,
                }));
            } else if depth + 1 < 2 {
                dirs.push((p, depth + 1));
            }
        }
    }
    let skip: Vec<globset::GlobMatcher> = located
        .config
        .git
        .skip_repos
        .iter()
        .filter_map(|g| globset::Glob::new(g).ok().map(|g| g.compile_matcher()))
        .collect();
    found.retain(|r| !skip.iter().any(|m| m.is_match(r.path.as_str())));
    found.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    // Disambiguate ids that collide (two `plugin-x` dirs at different depths).
    let mut ids: HashSet<String> = HashSet::new();
    for r in found.iter_mut() {
        if !ids.insert(r.id.0.clone()) {
            r.id = RepoId::new(r.path.as_str().replace('/', "__"));
            ids.insert(r.id.0.clone());
        }
    }
    found
}

pub use classify::ImporterFor;
pub use walk::{adr_files, feature_files};

#[allow(dead_code)]
fn _kinds(_k: ImporterKind) {}

/// Files matching one glob below the root (used by doctor / evidence discovery). Walks only
/// the literal prefix of the glob so `target/**` is not traversed beyond what is needed.
pub fn glob_files(root: &Path, pattern: &str) -> Vec<std::path::PathBuf> {
    let Ok(glob) = globset::Glob::new(pattern) else { return vec![] };
    let matcher = glob.compile_matcher();
    let prefix: String = pattern.split(['*', '?', '[']).next().unwrap_or("").to_string();
    let prefix_dir = if prefix.ends_with('/') {
        prefix.clone()
    } else {
        prefix.rsplit_once('/').map(|(d, _)| format!("{d}/")).unwrap_or_default()
    };
    let start = if prefix_dir.is_empty() {
        root.to_path_buf()
    } else {
        root.join(prefix_dir.trim_end_matches('/'))
    };
    if !start.exists() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut builder = ignore::WalkBuilder::new(&start);
    builder.standard_filters(false).hidden(false).follow_links(false).max_depth(Some(12));
    builder.filter_entry(|e| {
        let n = e.file_name().to_string_lossy();
        n != ".git" && n != "node_modules"
    });
    for entry in builder.build().flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        if let Some(rel) = relativize(root, p) {
            if matcher.is_match(rel.as_str()) {
                out.push(p.to_path_buf());
            }
        }
    }
    out.sort();
    out
}
