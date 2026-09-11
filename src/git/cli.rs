//! Process-based `GitBackend` using the `git` executable.

use super::parse::{LOG_FORMAT, count_status_entries, parse_log};
use super::{GitBackend, GitError, HeadInfo, RawCommit, TagInfo};
use crate::time::parse_timestamp;
use jiff::Timestamp;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Default, Clone)]
pub struct GitCli;

impl GitCli {
    fn run(&self, repo: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
        let out = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    GitError::NotInstalled
                } else {
                    GitError::Io(e)
                }
            })?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if stderr.contains("not a git repository") {
                return Err(GitError::NotARepo(repo.display().to_string()));
            }
            return Err(GitError::Command { cmd: args.join(" "), stderr });
        }
        Ok(out.stdout)
    }

    fn run_str(&self, repo: &Path, args: &[&str]) -> Result<String, GitError> {
        Ok(String::from_utf8_lossy(&self.run(repo, args)?).to_string())
    }
}

impl GitBackend for GitCli {
    fn head(&self, repo: &Path, check_dirty: bool) -> Result<Option<HeadInfo>, GitError> {
        let sha = match self.run_str(repo, &["rev-parse", "--verify", "-q", "HEAD"]) {
            Ok(s) => s.trim().to_string(),
            Err(GitError::Command { .. }) => return Ok(None), // unborn HEAD
            Err(e) => return Err(e),
        };
        if sha.is_empty() {
            return Ok(None);
        }
        let branch = self
            .run_str(repo, &["symbolic-ref", "--short", "-q", "HEAD"])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let dirty_files = if check_dirty {
            count_status_entries(
                &self.run(repo, &["status", "--porcelain=v1", "-z", "--untracked-files=normal"])?,
            )
        } else {
            0
        };
        Ok(Some(HeadInfo { sha, branch, dirty_files }))
    }

    fn log(
        &self,
        repo: &Path,
        range: Option<&str>,
        max: usize,
    ) -> Result<Vec<RawCommit>, GitError> {
        let format = format!("--format={LOG_FORMAT}");
        let max_s = max.to_string();
        let mut args = vec![
            "log",
            &format,
            "--numstat",
            "-M",
            "--no-color",
            "--date=iso-strict",
            "-n",
            &max_s,
        ];
        if let Some(r) = range {
            args.push(r);
        }
        args.push("--");
        let out = match self.run_str(repo, &args) {
            Ok(o) => o,
            Err(GitError::Command { stderr, .. })
                if stderr.contains("does not have any commits")
                    || stderr.contains("bad default revision") =>
            {
                return Ok(vec![]);
            }
            Err(e) => return Err(e),
        };
        Ok(parse_log(&out))
    }

    fn rev_list(&self, repo: &Path, from: &str, to: &str) -> Result<Vec<String>, GitError> {
        let range = format!("{from}..{to}");
        let out = self.run_str(repo, &["rev-list", &range, "--"])?;
        Ok(out.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
    }

    fn is_ancestor(&self, repo: &Path, ancestor: &str, descendant: &str) -> Result<bool, GitError> {
        let out = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["merge-base", "--is-ancestor", ancestor, descendant])
            .output()?;
        Ok(out.status.success())
    }

    fn tags(&self, repo: &Path) -> Result<Vec<TagInfo>, GitError> {
        let out = self.run_str(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname:short)\x1f%(objectname)\x1f%(*objectname)\x1f%(creatordate:iso-strict)",
                "refs/tags",
            ],
        )?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.split('\x1f').collect();
                if f.len() < 4 {
                    return None;
                }
                let sha = if f[2].is_empty() { f[1] } else { f[2] };
                Some(TagInfo {
                    name: f[0].to_string(),
                    sha: sha.to_string(),
                    at: parse_timestamp(f[3]),
                })
            })
            .collect())
    }

    fn last_commit_for_path(
        &self,
        repo: &Path,
        path: &str,
    ) -> Result<Option<(String, Timestamp)>, GitError> {
        let out = self.run_str(repo, &["log", "-1", "--format=%H %cI", "--", path])?;
        let line = out.trim();
        if line.is_empty() {
            return Ok(None);
        }
        let (sha, date) = line.split_once(' ').unwrap_or((line, ""));
        Ok(parse_timestamp(date).map(|t| (sha.to_string(), t)))
    }

    fn commit_count(&self, repo: &Path) -> Result<u64, GitError> {
        match self.run_str(repo, &["rev-list", "--count", "HEAD"]) {
            Ok(s) => Ok(s.trim().parse().unwrap_or(0)),
            Err(GitError::Command { .. }) => Ok(0),
            Err(e) => Err(e),
        }
    }

    fn version(&self) -> Result<String, GitError> {
        let out =
            Command::new("git").arg("--version").output().map_err(|_| GitError::NotInstalled)?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@x")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@x")
            .env("GIT_AUTHOR_DATE", "2026-01-02T03:04:05Z")
            .env("GIT_COMMITTER_DATE", "2026-01-02T03:04:05Z")
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    #[test]
    fn reads_a_temp_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        git(d, &["init", "-q", "-b", "main"]);
        let g = GitCli;
        assert!(g.head(d, true).unwrap().is_none(), "unborn head");
        assert!(g.log(d, None, 10).unwrap().is_empty());
        std::fs::write(d.join("a.txt"), "hello\n").unwrap();
        git(d, &["add", "."]);
        git(d, &["commit", "-q", "-m", "first [FEAT-1]", "-m", "body\n\nCo-Authored-By: X <x@y>"]);
        git(d, &["tag", "v0.1.0"]);
        std::fs::write(d.join("a.txt"), "hello\nworld\n").unwrap();
        let head = g.head(d, true).unwrap().unwrap();
        assert_eq!(head.branch.as_deref(), Some("main"));
        assert_eq!(head.dirty_files, 1);
        let log = g.log(d, None, 10).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].subject, "first [FEAT-1]");
        assert_eq!(log[0].files[0].path, "a.txt");
        assert_eq!(log[0].trailers[0].0, "Co-Authored-By");
        assert!(log[0].refs.iter().any(|r| r == "tag: v0.1.0"));
        assert_eq!(g.tags(d).unwrap()[0].name, "v0.1.0");
        assert_eq!(g.commit_count(d).unwrap(), 1);
        assert!(g.last_commit_for_path(d, "a.txt").unwrap().is_some());
    }
}
