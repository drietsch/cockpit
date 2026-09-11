//! Git access. Cockpit shells out to the `git` CLI behind the `GitBackend` trait so that the
//! backend can be swapped (git2 / gix) without touching callers, and so that tests can use a
//! fake graph.

pub mod cli;
pub mod parse;

use crate::ids::Sha;
use jiff::Timestamp;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git is not installed or not on PATH")]
    NotInstalled,
    #[error("{0} is not a git repository")]
    NotARepo(String),
    #[error("git {cmd} failed: {stderr}")]
    Command { cmd: String, stderr: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// A commit as git reports it: repo-relative paths, raw refs, raw trailers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawCommit {
    pub sha: String,
    pub parents: Vec<String>,
    pub author: String,
    pub author_email: String,
    pub authored_at: Timestamp,
    pub committed_at: Timestamp,
    pub subject: String,
    pub body: String,
    pub refs: Vec<String>,
    pub trailers: Vec<(String, String)>,
    pub files: Vec<RawFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawFile {
    pub path: String,
    pub rename_from: Option<String>,
    pub added: Option<u32>,
    pub deleted: Option<u32>,
    pub binary: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadInfo {
    pub sha: String,
    pub branch: Option<String>,
    pub dirty_files: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagInfo {
    pub name: String,
    pub sha: String,
    pub at: Option<Timestamp>,
}

pub trait GitBackend: Send + Sync {
    /// `None` when the directory has no commits yet (unborn HEAD).
    fn head(&self, repo: &Path, check_dirty: bool) -> Result<Option<HeadInfo>, GitError>;
    /// Newest first. `range` is a git revision range such as `abc..HEAD`; `None` = from HEAD.
    fn log(&self, repo: &Path, range: Option<&str>, max: usize)
    -> Result<Vec<RawCommit>, GitError>;
    fn rev_list(&self, repo: &Path, from: &str, to: &str) -> Result<Vec<String>, GitError>;
    fn is_ancestor(&self, repo: &Path, ancestor: &str, descendant: &str) -> Result<bool, GitError>;
    fn tags(&self, repo: &Path) -> Result<Vec<TagInfo>, GitError>;
    fn last_commit_for_path(
        &self,
        repo: &Path,
        path: &str,
    ) -> Result<Option<(String, Timestamp)>, GitError>;
    fn commit_count(&self, repo: &Path) -> Result<u64, GitError>;
    fn version(&self) -> Result<String, GitError>;
}

/// In-memory ancestry over the imported commits, for freshness comparisons that must not
/// spawn a process per evidence item.
#[derive(Default, Debug)]
pub struct CommitGraph {
    parents: HashMap<Sha, Vec<Sha>>,
    committed_at: HashMap<Sha, Timestamp>,
}

impl CommitGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, sha: Sha, parents: Vec<Sha>, committed_at: Timestamp) {
        self.committed_at.insert(sha.clone(), committed_at);
        self.parents.insert(sha, parents);
    }

    pub fn contains(&self, sha: &Sha) -> bool {
        self.parents.contains_key(sha)
    }

    pub fn committed_at(&self, sha: &Sha) -> Option<Timestamp> {
        self.committed_at.get(sha).copied()
    }

    /// Is `ancestor` reachable from `descendant` (or equal)? Bounded BFS over parents, pruning
    /// commits older than the ancestor minus an hour of clock-skew tolerance.
    pub fn is_ancestor(&self, ancestor: &Sha, descendant: &Sha) -> bool {
        if ancestor == descendant {
            return true;
        }
        let Some(floor) = self.committed_at.get(ancestor).map(|t| t.as_second() - 3_600) else {
            return false;
        };
        let mut seen: HashSet<&Sha> = HashSet::new();
        let mut queue: VecDeque<&Sha> = VecDeque::new();
        queue.push_back(descendant);
        let mut steps = 0usize;
        while let Some(cur) = queue.pop_front() {
            steps += 1;
            if steps > 200_000 {
                return false;
            }
            let Some(parents) = self.parents.get(cur) else { continue };
            for p in parents {
                if p == ancestor {
                    return true;
                }
                if seen.insert(p) {
                    if let Some(t) = self.committed_at.get(p) {
                        if t.as_second() < floor {
                            continue;
                        }
                    }
                    queue.push_back(p);
                }
            }
        }
        false
    }

    /// Commits reachable from `to` but not from `from` (`from..to`), newest first, bounded.
    pub fn between(&self, from: &Sha, to: &Sha, limit: usize) -> Vec<Sha> {
        let mut excluded: HashSet<Sha> = HashSet::new();
        let mut queue: VecDeque<&Sha> = VecDeque::new();
        queue.push_back(from);
        excluded.insert(from.clone());
        while let Some(cur) = queue.pop_front() {
            if excluded.len() > 20_000 {
                break;
            }
            for p in self.parents.get(cur).into_iter().flatten() {
                if excluded.insert(p.clone()) {
                    queue.push_back(p);
                }
            }
        }
        let mut out: Vec<Sha> = Vec::new();
        let mut seen: HashSet<Sha> = HashSet::new();
        let mut queue: VecDeque<&Sha> = VecDeque::new();
        queue.push_back(to);
        seen.insert(to.clone());
        while let Some(cur) = queue.pop_front() {
            if excluded.contains(cur) {
                continue;
            }
            out.push(cur.clone());
            if out.len() >= limit {
                break;
            }
            for p in self.parents.get(cur).into_iter().flatten() {
                if seen.insert(p.clone()) {
                    queue.push_back(p);
                }
            }
        }
        out.sort_by(|a, b| self.committed_at.get(b).cmp(&self.committed_at.get(a)));
        out
    }

    /// Latest commit (by committed_at) among `candidates`, tie-broken by descent.
    pub fn newest<'a>(&self, candidates: impl Iterator<Item = &'a Sha>) -> Option<Sha> {
        let mut best: Option<(&Sha, Timestamp)> = None;
        for c in candidates {
            let Some(t) = self.committed_at.get(c) else { continue };
            match best {
                None => best = Some((c, *t)),
                Some((b, bt)) => {
                    if *t > bt || (*t == bt && self.is_ancestor(b, c)) {
                        best = Some((c, *t));
                    }
                }
            }
        }
        best.map(|(s, _)| s.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Timestamp {
        crate::time::parse_timestamp(s).unwrap()
    }

    #[test]
    fn ancestry() {
        let mut g = CommitGraph::new();
        g.insert("a".into(), vec![], ts("2026-01-01T00:00:00Z"));
        g.insert("b".into(), vec!["a".into()], ts("2026-01-02T00:00:00Z"));
        g.insert("c".into(), vec!["b".into()], ts("2026-01-03T00:00:00Z"));
        g.insert("x".into(), vec!["a".into()], ts("2026-01-02T12:00:00Z"));
        assert!(g.is_ancestor(&"a".into(), &"c".into()));
        assert!(g.is_ancestor(&"b".into(), &"c".into()));
        assert!(!g.is_ancestor(&"c".into(), &"b".into()));
        assert!(!g.is_ancestor(&"x".into(), &"c".into()));
        assert_eq!(
            g.newest([Sha::from("b"), Sha::from("c"), Sha::from("x")].iter()),
            Some(Sha::from("c"))
        );
    }
}
