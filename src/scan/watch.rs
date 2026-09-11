//! Change detection for `serve --watch`. A polling watcher (mtimes of documents, evidence
//! files and each repo's `.git` refs) keeps the dependency surface small and behaves the same
//! on every platform; a native watcher can replace it behind the same `take_changes()`.

use crate::config::Located;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

pub struct Watcher {
    located: Located,
    snapshot: HashMap<PathBuf, SystemTime>,
    last_poll: Instant,
    interval: Duration,
    /// Debounce: report only when the tree has been quiet for this long.
    quiet_for: Duration,
    changed_since: Option<Instant>,
}

impl Watcher {
    pub fn start(located: &Located) -> anyhow::Result<Watcher> {
        let mut w = Watcher {
            located: located.clone(),
            snapshot: HashMap::new(),
            last_poll: Instant::now(),
            interval: Duration::from_millis(1000),
            quiet_for: Duration::from_millis(250),
            changed_since: None,
        };
        w.snapshot = w.take_snapshot();
        Ok(w)
    }

    fn watched_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        files.extend(crate::scan::feature_files(&self.located));
        files.extend(crate::scan::adr_files(&self.located));
        for src in self.located.evidence_sources() {
            for pat in src.paths() {
                files.extend(crate::scan::glob_files(&self.located.root, &pat));
            }
        }
        for repo in crate::scan::discover_repos(&self.located) {
            let git = repo.path.to_abs(&self.located.root).join(".git");
            for name in ["HEAD", "index", "packed-refs", "logs/HEAD", "ORIG_HEAD"] {
                files.push(git.join(name));
            }
            if let Ok(rd) = std::fs::read_dir(git.join("refs/heads")) {
                for e in rd.flatten() {
                    files.push(e.path());
                }
            }
        }
        if let Some(c) = &self.located.config_path {
            files.push(c.clone());
        }
        files
    }

    fn take_snapshot(&self) -> HashMap<PathBuf, SystemTime> {
        let mut m = HashMap::new();
        for f in self.watched_files() {
            if let Ok(meta) = std::fs::metadata(&f) {
                if let Ok(t) = meta.modified() {
                    m.insert(f, t);
                }
            }
        }
        m
    }

    /// True when something changed and the burst has settled.
    pub fn take_changes(&mut self) -> bool {
        if self.last_poll.elapsed() < self.interval {
            return false;
        }
        self.last_poll = Instant::now();
        let now = self.take_snapshot();
        if now != self.snapshot {
            self.snapshot = now;
            self.changed_since = Some(Instant::now());
            return false;
        }
        match self.changed_since {
            Some(t) if t.elapsed() >= self.quiet_for => {
                self.changed_since = None;
                true
            }
            _ => false,
        }
    }
}
