//! The `Cockpit` facade: open a project, scan it, get its state. Used by the CLI and server.

use crate::config::{Config, ConfigError, Located};
use crate::git::GitBackend;
use crate::git::cli::GitCli;
use crate::scan::{ProgressFn, ScanReport, Scanner};
use crate::state::build::{BuildInput, build};
use crate::state::{ProjectState, StateHandle};
use crate::store::Store;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum CockpitError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Store(#[from] crate::store::StoreError),
    #[error(transparent)]
    Scan(#[from] crate::scan::ScanError),
}

pub struct Cockpit {
    pub located: Located,
    pub git: Box<dyn GitBackend>,
    pub store: Store,
    pub state: StateHandle,
    last_report: Option<ScanReport>,
}

impl Cockpit {
    pub fn open(
        cwd: &Path,
        project: Option<&Path>,
        index_override: Option<&Path>,
    ) -> Result<Cockpit, CockpitError> {
        let located = Config::discover(cwd, project)?;
        Self::from_located(located, index_override)
    }

    pub fn from_located(
        located: Located,
        index_override: Option<&Path>,
    ) -> Result<Cockpit, CockpitError> {
        let index = index_override.map(|p| p.to_path_buf()).unwrap_or_else(|| located.index_path());
        let store = Store::open(&index)?;
        let git: Box<dyn GitBackend> = Box::new(GitCli);
        let repos = crate::scan::discover_repos(&located);
        let scanner = Scanner::new(&located, git.as_ref());
        let project = scanner.project(&repos);
        let state = StateHandle::new(ProjectState::empty(project));
        let mut c = Cockpit { located, git, store, state, last_report: None };
        // If an index exists from a previous run, expose its state immediately.
        if c.store.meta("last_scan_at")?.is_some() {
            let _ = c.rebuild_state();
        }
        Ok(c)
    }

    pub fn scan(&mut self, progress: Option<&ProgressFn<'_>>) -> Result<ScanReport, CockpitError> {
        let mut scanner = Scanner::new(&self.located, self.git.as_ref());
        if let Some(p) = progress {
            scanner = scanner.with_progress(p);
        }
        let report = scanner.full(&mut self.store)?;
        self.last_report = Some(report.clone());
        let t = std::time::Instant::now();
        self.rebuild_state()?;
        tracing::debug!("state rebuilt in {} ms", t.elapsed().as_millis());
        Ok(report)
    }

    pub fn rebuild_state(&mut self) -> Result<(), CockpitError> {
        let repos = self.store.repos()?;
        let scanner = Scanner::new(&self.located, self.git.as_ref());
        let project = scanner.project(&repos);
        let prev = self.state.load();
        let state = build(BuildInput {
            store: &self.store,
            located: &self.located,
            project,
            previous: Some(&prev),
            last_scan_duration_ms: self.last_report.as_ref().map(|r| r.duration_ms),
        })?;
        self.state.store(state);
        Ok(())
    }

    pub fn last_report(&self) -> Option<&ScanReport> {
        self.last_report.as_ref()
    }
}
