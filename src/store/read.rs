//! Reads. Bounded history loaders keep the in-memory snapshot small.

use super::{Result, Store};
use crate::hash::ContentHash;
use crate::ids::*;
use crate::model::*;
use crate::paths::RelPath;
use crate::time::from_millis;
use jiff::Timestamp;
use rusqlite::params;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ScanFileInfo {
    pub mtime: Timestamp,
    pub size: u64,
    pub content_hash: String,
    pub importer: String,
    pub importer_version: u32,
}

fn load_json<T: serde::de::DeserializeOwned>(store: &Store, sql: &str) -> Result<Vec<T>> {
    let mut stmt = store.conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        let s = row?;
        match serde_json::from_str::<T>(&s) {
            Ok(v) => out.push(v),
            Err(e) => tracing::warn!("skipping unreadable row: {e}"),
        }
    }
    Ok(out)
}

impl Store {
    pub fn scan_files(&self) -> Result<HashMap<RelPath, ScanFileInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, mtime_ms, size, content_hash, importer, importer_version FROM scan_files",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                RelPath::new(r.get::<_, String>(0)?),
                ScanFileInfo {
                    mtime: from_millis(r.get(1)?),
                    size: r.get::<_, i64>(2)? as u64,
                    content_hash: r.get(3)?,
                    importer: r.get(4)?,
                    importer_version: r.get::<_, i64>(5)? as u32,
                },
            ))
        })?;
        let mut m = HashMap::new();
        for r in rows {
            let (p, i) = r?;
            m.insert(p, i);
        }
        Ok(m)
    }

    pub fn scan_file_hash(&self, path: &RelPath) -> Result<Option<ContentHash>> {
        Ok(self.scan_files()?.get(path).and_then(|i| ContentHash::from_hex(&i.content_hash)))
    }

    pub fn repos(&self) -> Result<Vec<Repo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, primary_branch, head_sha, head_branch, dirty, dirty_files, commit_count, last_synced_at, note FROM repos ORDER BY path",
        )?;
        let rows = stmt.query_map([], |r| {
            let head_sha: Option<String> = r.get(3)?;
            let synced: Option<i64> = r.get(8)?;
            Ok(Repo {
                id: RepoId::new(r.get::<_, String>(0)?),
                path: RelPath::new(r.get::<_, String>(1)?),
                primary_branch: r.get(2)?,
                head: head_sha.map(|sha| RepoHead {
                    sha: Sha::new(sha),
                    branch: r.get(4).ok().flatten(),
                    dirty: r.get::<_, i64>(5).unwrap_or(0) != 0,
                    dirty_files: r.get::<_, i64>(6).unwrap_or(0) as u32,
                    observed_at: from_millis(synced.unwrap_or(0)),
                }),
                commit_count: r.get::<_, i64>(7)? as u64,
                note: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn features(&self) -> Result<Vec<Feature>> {
        load_json(self, "SELECT data_json FROM features ORDER BY id")
    }

    pub fn adrs(&self) -> Result<Vec<Adr>> {
        load_json(self, "SELECT data_json FROM adrs ORDER BY number, id")
    }

    pub fn commits(&self, repo: &RepoId, limit: usize) -> Result<Vec<Commit>> {
        let mut stmt = self.conn.prepare(
            "SELECT data_json FROM commits WHERE repo = ?1 ORDER BY committed_at DESC LIMIT ?2",
        )?;
        let rows =
            stmt.query_map(params![repo.as_str(), limit as i64], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str(&row?)?);
        }
        Ok(out)
    }

    /// Compact ancestry data for every commit of every repo: (repo, sha, parents, committed_at).
    pub fn commit_graph_rows(&self) -> Result<Vec<(RepoId, Sha, Vec<Sha>, Timestamp)>> {
        let commits: Vec<Commit> = load_json(self, "SELECT data_json FROM commits")?;
        Ok(commits
            .into_iter()
            .map(|c| (c.key.repo, c.key.sha, c.parents, c.committed_at))
            .collect())
    }

    pub fn all_commits(&self) -> Result<Vec<Commit>> {
        load_json(self, "SELECT data_json FROM commits ORDER BY committed_at DESC")
    }

    pub fn known_shas(&self, repo: &RepoId) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT sha FROM commits WHERE repo = ?1")?;
        let rows = stmt.query_map(params![repo.as_str()], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn warnings(&self) -> Result<Vec<ImportWarning>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, importer, message, location, fatal FROM warnings ORDER BY path",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ImportWarning {
                path: RelPath::new(r.get::<_, String>(0)?),
                importer: r.get(1)?,
                message: r.get(2)?,
                location: r.get(3)?,
                fatal: r.get::<_, i64>(4)? != 0,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn suites(&self) -> Result<Vec<Suite>> {
        load_json(self, "SELECT data_json FROM suites ORDER BY id")
    }

    pub fn tests(&self) -> Result<Vec<Test>> {
        load_json(self, "SELECT data_json FROM tests ORDER BY id")
    }

    pub fn runs(&self) -> Result<Vec<Run>> {
        load_json(self, "SELECT data_json FROM runs ORDER BY finished_at DESC")
    }

    /// The last `n` runs of every test, newest first per test.
    pub fn recent_test_runs(&self, n: usize) -> Result<Vec<TestRun>> {
        load_json(
            self,
            &format!(
                "SELECT data_json FROM (SELECT data_json, ROW_NUMBER() OVER (PARTITION BY test_id ORDER BY timestamp DESC, id) AS rn FROM test_runs) WHERE rn <= {n}"
            ),
        )
    }

    pub fn benchmarks(&self) -> Result<Vec<Benchmark>> {
        load_json(self, "SELECT data_json FROM benchmarks ORDER BY name")
    }

    pub fn recent_benchmark_runs(&self, n: usize) -> Result<Vec<BenchmarkRun>> {
        load_json(
            self,
            &format!(
                "SELECT data_json FROM (SELECT data_json, ROW_NUMBER() OVER (PARTITION BY benchmark_id, env_class_id ORDER BY timestamp DESC, id) AS rn FROM benchmark_runs) WHERE rn <= {n}"
            ),
        )
    }

    pub fn environments(&self) -> Result<Vec<EnvironmentFingerprint>> {
        load_json(self, "SELECT data_json FROM environments")
    }

    /// Latest two evidence rows per visual check.
    pub fn recent_visuals(&self) -> Result<Vec<VisualEvidence>> {
        load_json(
            self,
            "SELECT data_json FROM (SELECT data_json, ROW_NUMBER() OVER (PARTITION BY check_id ORDER BY timestamp DESC, id) AS rn FROM visual_evidence) WHERE rn <= 2",
        )
    }

    pub fn artifacts(&self) -> Result<Vec<Artifact>> {
        load_json(self, "SELECT data_json FROM artifacts")
    }

    pub fn relationships(&self) -> Result<Vec<Relationship>> {
        load_json(self, "SELECT data_json FROM relationships")
    }

    pub fn unresolved_refs(&self) -> Result<Vec<UnresolvedRef>> {
        load_json(self, "SELECT data_json FROM unresolved_refs")
    }

    pub fn feature_health(&self) -> Result<Vec<(FeatureId, FeatureHealth)>> {
        let mut stmt = self.conn.prepare("SELECT feature_id, data_json FROM feature_health")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (id, json) = row?;
            if let Ok(h) = serde_json::from_str(&json) {
                out.push((FeatureId::new(id), h));
            }
        }
        Ok(out)
    }

    pub fn attention_items(&self, only_open: bool) -> Result<Vec<AttentionItem>> {
        load_json(
            self,
            if only_open {
                "SELECT data_json FROM attention_items WHERE status = 'open' ORDER BY score DESC, first_seen DESC"
            } else {
                "SELECT data_json FROM attention_items ORDER BY score DESC, first_seen DESC"
            },
        )
    }

    pub fn events(&self, limit: usize) -> Result<Vec<EngineeringEvent>> {
        load_json(self, &format!("SELECT data_json FROM events ORDER BY at DESC LIMIT {limit}"))
    }
}
