//! Writes. Every upsert is idempotent; history rows are `INSERT OR IGNORE`d on their identity.

use super::{Result, Store};
use crate::hash::ContentHash;
use crate::ids::*;
use crate::model::*;
use crate::paths::RelPath;
use crate::time::millis;
use jiff::Timestamp;
use rusqlite::params;

#[derive(Debug, Clone)]
pub struct ScanFileRecord {
    pub path: RelPath,
    pub mtime: Timestamp,
    pub size: u64,
    pub content_hash: ContentHash,
    pub importer: String,
    pub importer_version: u32,
    pub source_id: Option<String>,
    pub status: String,
    pub warnings: Vec<ImportWarning>,
}

impl Store {
    pub fn begin(&mut self) -> Result<rusqlite::Transaction<'_>> {
        Ok(self.conn.transaction()?)
    }

    pub fn upsert_repo(&self, repo: &Repo, now: Timestamp) -> Result<()> {
        self.conn.execute(
            "INSERT INTO repos(id, path, primary_branch, head_sha, head_branch, dirty, dirty_files, commit_count, last_synced_at, note)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET path = excluded.path, primary_branch = excluded.primary_branch,
               head_sha = excluded.head_sha, head_branch = excluded.head_branch, dirty = excluded.dirty,
               dirty_files = excluded.dirty_files, commit_count = excluded.commit_count,
               last_synced_at = excluded.last_synced_at, note = excluded.note",
            params![
                repo.id.as_str(),
                repo.path.as_str(),
                repo.primary_branch,
                repo.head.as_ref().map(|h| h.sha.as_str().to_string()),
                repo.head.as_ref().and_then(|h| h.branch.clone()),
                repo.head.as_ref().map(|h| h.dirty as i64).unwrap_or(0),
                repo.head.as_ref().map(|h| h.dirty_files as i64).unwrap_or(0),
                repo.commit_count as i64,
                millis(now),
                repo.note,
            ],
        )?;
        Ok(())
    }

    pub fn upsert_feature(
        &self,
        f: &Feature,
        content_hash: &ContentHash,
        now: Timestamp,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO features(id, data_json, title, lifecycle, importance, source_path, content_hash, first_seen, last_seen)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(id) DO UPDATE SET data_json = excluded.data_json, title = excluded.title,
               lifecycle = excluded.lifecycle, importance = excluded.importance, source_path = excluded.source_path,
               content_hash = excluded.content_hash, last_seen = excluded.last_seen",
            params![
                f.id.as_str(),
                serde_json::to_string(f)?,
                f.title,
                f.lifecycle.as_str(),
                f.importance.as_str(),
                f.source.as_str(),
                content_hash.hex(),
                millis(now),
            ],
        )?;
        Ok(())
    }

    pub fn upsert_adr(&self, a: &Adr, content_hash: &ContentHash, now: Timestamp) -> Result<()> {
        self.conn.execute(
            "INSERT INTO adrs(id, data_json, number, title, status, source_path, content_hash, first_seen, last_seen)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(id) DO UPDATE SET data_json = excluded.data_json, number = excluded.number, title = excluded.title,
               status = excluded.status, source_path = excluded.source_path, content_hash = excluded.content_hash,
               last_seen = excluded.last_seen",
            params![
                a.id.as_str(),
                serde_json::to_string(a)?,
                a.number.map(|n| n as i64),
                a.title,
                a.status.as_str(),
                a.source.as_str(),
                content_hash.hex(),
                millis(now),
            ],
        )?;
        Ok(())
    }

    /// Remove cache entities that came from a source file that no longer exists.
    pub fn delete_by_source_path(&self, path: &RelPath) -> Result<()> {
        self.conn.execute("DELETE FROM features WHERE source_path = ?1", params![path.as_str()])?;
        self.conn.execute("DELETE FROM adrs WHERE source_path = ?1", params![path.as_str()])?;
        self.conn.execute("DELETE FROM scan_files WHERE path = ?1", params![path.as_str()])?;
        self.conn.execute("DELETE FROM warnings WHERE path = ?1", params![path.as_str()])?;
        Ok(())
    }

    pub fn upsert_commit(&self, c: &Commit) -> Result<()> {
        self.conn.execute(
            "INSERT INTO commits(repo, sha, data_json, committed_at, subject) VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(repo, sha) DO UPDATE SET data_json = excluded.data_json, committed_at = excluded.committed_at,
               subject = excluded.subject",
            params![c.key.repo.as_str(), c.key.sha.as_str(), serde_json::to_string(c)?, millis(c.committed_at), c.subject],
        )?;
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR IGNORE INTO commit_files(repo, sha, path) VALUES(?1, ?2, ?3)",
        )?;
        for f in &c.files {
            stmt.execute(params![c.key.repo.as_str(), c.key.sha.as_str(), f.path.as_str()])?;
        }
        Ok(())
    }

    pub fn delete_commits_of_repo(&self, repo: &RepoId) -> Result<()> {
        self.conn.execute("DELETE FROM commit_files WHERE repo = ?1", params![repo.as_str()])?;
        self.conn.execute("DELETE FROM commits WHERE repo = ?1", params![repo.as_str()])?;
        Ok(())
    }

    pub fn upsert_scan_file(&self, r: &ScanFileRecord, now: Timestamp) -> Result<()> {
        self.conn.execute(
            "INSERT INTO scan_files(path, mtime_ms, size, content_hash, importer, importer_version, source_id, last_scanned_at, status, warnings_json)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(path) DO UPDATE SET mtime_ms = excluded.mtime_ms, size = excluded.size, content_hash = excluded.content_hash,
               importer = excluded.importer, importer_version = excluded.importer_version, source_id = excluded.source_id,
               last_scanned_at = excluded.last_scanned_at, status = excluded.status, warnings_json = excluded.warnings_json",
            params![
                r.path.as_str(),
                millis(r.mtime),
                r.size as i64,
                r.content_hash.hex(),
                r.importer,
                r.importer_version as i64,
                r.source_id,
                millis(now),
                r.status,
                serde_json::to_string(&r.warnings)?,
            ],
        )?;
        Ok(())
    }

    pub fn forget_scan_file(&self, path: &RelPath) -> Result<()> {
        self.conn.execute("DELETE FROM scan_files WHERE path = ?1", params![path.as_str()])?;
        self.conn.execute("DELETE FROM warnings WHERE path = ?1", params![path.as_str()])?;
        Ok(())
    }

    pub fn replace_warnings(
        &self,
        path: &RelPath,
        warnings: &[ImportWarning],
        now: Timestamp,
    ) -> Result<()> {
        self.conn.execute("DELETE FROM warnings WHERE path = ?1", params![path.as_str()])?;
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO warnings(path, importer, message, location, fatal, seen_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for w in warnings {
            stmt.execute(params![
                w.path.as_str(),
                w.importer,
                w.message,
                w.location,
                w.fatal as i64,
                millis(now)
            ])?;
        }
        Ok(())
    }

    pub fn upsert_suite(&self, s: &Suite) -> Result<()> {
        self.conn.execute(
            "INSERT INTO suites(id, data_json, source_id, last_seen, run_count) VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET data_json = excluded.data_json, source_id = excluded.source_id,
               last_seen = MAX(COALESCE(suites.last_seen, 0), COALESCE(excluded.last_seen, 0)), run_count = excluded.run_count",
            params![s.id.as_str(), serde_json::to_string(s)?, s.source.as_str(), s.last_seen.map(millis), s.run_count as i64],
        )?;
        Ok(())
    }

    pub fn upsert_test(&self, t: &Test) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tests(id, data_json, suite_id, test_key, source_path, first_seen, last_seen) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET data_json = excluded.data_json, source_path = excluded.source_path,
               last_seen = MAX(tests.last_seen, excluded.last_seen)",
            params![
                t.id.as_str(),
                serde_json::to_string(t)?,
                t.suite.as_str(),
                t.key,
                t.source_path.as_ref().map(|p| p.as_str().to_string()),
                millis(t.first_seen),
                millis(t.last_seen),
            ],
        )?;
        Ok(())
    }

    /// Insert a run batch. Returns `false` (and inserts nothing) when the run identity already
    /// exists, which is what makes re-scanning the same file a no-op.
    pub fn insert_run(&self, run: &Run, identity: &str) -> Result<bool> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO runs(id, data_json, source_id, suite_id, kind, identity, repo, commit_sha, branch, finished_at, source_path, silent, imported_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                run.id.as_str(),
                serde_json::to_string(run)?,
                run.source.as_str(),
                run.suite.as_ref().map(|s| s.as_str().to_string()),
                run.kind.as_str(),
                identity,
                run.provenance.commit.as_ref().map(|c| c.key.repo.as_str().to_string()),
                run.provenance.commit.as_ref().map(|c| c.key.sha.as_str().to_string()),
                run.provenance.branch,
                millis(run.finished_at.value),
                run.provenance.source_path.as_str(),
                run.silent as i64,
                millis(run.provenance.imported_at),
            ],
        )?;
        Ok(n > 0)
    }

    pub fn insert_test_run(&self, tr: &TestRun) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO test_runs(id, data_json, run_id, test_id, status, duration_ms, timestamp) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                tr.id.as_str(),
                serde_json::to_string(tr)?,
                tr.run.as_str(),
                tr.test.as_str(),
                tr.status.as_str(),
                tr.duration_ms.map(|d| d as i64),
                millis(tr.timestamp.value),
            ],
        )?;
        Ok(())
    }

    pub fn upsert_environment(&self, e: &EnvironmentFingerprint, now: Timestamp) -> Result<()> {
        self.conn.execute(
            "INSERT INTO environments(id, data_json, class_id, first_seen, last_seen) VALUES(?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(id) DO UPDATE SET last_seen = excluded.last_seen",
            params![e.id.as_str(), serde_json::to_string(e)?, e.class.as_str(), millis(now)],
        )?;
        Ok(())
    }

    pub fn upsert_benchmark(&self, b: &Benchmark) -> Result<()> {
        self.conn.execute(
            "INSERT INTO benchmarks(id, data_json, source_id, name, first_seen, last_seen) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET data_json = excluded.data_json, first_seen = MIN(benchmarks.first_seen, excluded.first_seen), last_seen = MAX(benchmarks.last_seen, excluded.last_seen)",
            params![b.id.as_str(), serde_json::to_string(b)?, b.source.as_str(), b.name, millis(b.first_seen), millis(b.last_seen)],
        )?;
        Ok(())
    }

    pub fn insert_benchmark_run(&self, br: &BenchmarkRun) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO benchmark_runs(id, data_json, run_id, benchmark_id, env_class_id, value, timestamp) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                br.id.as_str(),
                serde_json::to_string(br)?,
                br.run.as_str(),
                br.benchmark.as_str(),
                br.env_class.as_str(),
                br.measurement.value,
                millis(br.timestamp.value),
            ],
        )?;
        Ok(())
    }

    pub fn insert_visual(&self, v: &VisualEvidence) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO visual_evidence(id, data_json, check_id, run_id, status, timestamp) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![v.id.as_str(), serde_json::to_string(v)?, v.check.as_str(), v.run.as_str(), v.status.as_str(), millis(v.timestamp.value)],
        )?;
        Ok(())
    }

    pub fn upsert_artifact(&self, a: &Artifact, now: Timestamp) -> Result<()> {
        self.conn.execute(
            "INSERT INTO artifacts(id, data_json, last_seen) VALUES(?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET data_json = excluded.data_json, last_seen = excluded.last_seen",
            params![a.id.as_str(), serde_json::to_string(a)?, millis(now)],
        )?;
        Ok(())
    }

    pub fn replace_relationships(
        &self,
        rels: &[Relationship],
        unresolved: &[UnresolvedRef],
    ) -> Result<()> {
        self.conn.execute_batch("DELETE FROM relationships; DELETE FROM unresolved_refs;")?;
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR IGNORE INTO relationships(from_kind, from_id, to_kind, to_id, rel, origin_key, data_json) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for r in rels {
            stmt.execute(params![
                r.from.kind_str(),
                r.from.id_str(),
                r.to.kind_str(),
                r.to.id_str(),
                r.kind.as_str(),
                r.origin.key(),
                serde_json::to_string(r)?
            ])?;
        }
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR IGNORE INTO unresolved_refs(from_kind, from_id, raw, data_json) VALUES(?1, ?2, ?3, ?4)",
        )?;
        for u in unresolved {
            stmt.execute(params![
                u.from.kind_str(),
                u.from.id_str(),
                u.raw,
                serde_json::to_string(u)?
            ])?;
        }
        Ok(())
    }

    pub fn upsert_feature_health(&self, id: &FeatureId, h: &FeatureHealth) -> Result<()> {
        self.conn.execute(
            "INSERT INTO feature_health(feature_id, data_json, state, evaluated_at) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(feature_id) DO UPDATE SET data_json = excluded.data_json, state = excluded.state, evaluated_at = excluded.evaluated_at",
            params![id.as_str(), serde_json::to_string(h)?, h.state.as_str(), millis(h.evaluated_at)],
        )?;
        Ok(())
    }

    pub fn upsert_attention(&self, a: &AttentionItem) -> Result<()> {
        self.conn.execute(
            "INSERT INTO attention_items(id, data_json, status, feature_id, score, first_seen, last_seen) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET data_json = excluded.data_json, status = excluded.status, feature_id = excluded.feature_id,
               score = excluded.score, last_seen = excluded.last_seen",
            params![
                a.id.as_str(),
                serde_json::to_string(a)?,
                match a.status { AttentionStatus::Open => "open", AttentionStatus::Resolved => "resolved" },
                a.feature.as_ref().map(|f| f.as_str().to_string()),
                a.score,
                millis(a.first_seen),
                millis(a.last_seen),
            ],
        )?;
        Ok(())
    }

    /// Insert if new; `Ok(true)` when the row did not exist before.
    pub fn insert_event(&self, e: &EngineeringEvent) -> Result<bool> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO events(id, data_json, kind, at, feature_id) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                e.id.as_str(),
                serde_json::to_string(e)?,
                e.kind.as_str(),
                millis(e.at),
                e.feature.as_ref().map(|f| f.as_str().to_string())
            ],
        )?;
        Ok(n > 0)
    }
}

impl Store {
    /// Persist a test batch. Suites and tests are upserted; the run and its children are
    /// inserted only when the run identity is new. Returns whether the run was new.
    pub fn persist_test_batch(
        &self,
        b: &crate::import::TestBatch,
        identity: &str,
        now: Timestamp,
    ) -> Result<bool> {
        let new_run = self.insert_run(&b.run, identity)?;
        // Suite: keep the newest last_seen and count runs.
        let mut suite = b.suite.clone();
        if let Some(existing) = self.suites()?.into_iter().find(|s| s.id == suite.id) {
            suite.run_count = existing.run_count + if new_run { 1 } else { 0 };
            if let (Some(a), Some(e)) = (suite.last_seen, existing.last_seen) {
                if e > a {
                    suite.last_seen = Some(e);
                    suite.last_run = existing.last_run.clone();
                }
            }
        }
        self.upsert_suite(&suite)?;
        for t in &b.tests {
            self.upsert_test(t)?;
        }
        for a in &b.artifacts {
            self.upsert_artifact(a, now)?;
        }
        if new_run {
            for tr in &b.test_runs {
                self.insert_test_run(tr)?;
            }
            for v in &b.visuals {
                self.insert_visual(v)?;
            }
        }
        Ok(new_run)
    }

    pub fn persist_benchmark_batch(
        &self,
        b: &crate::import::BenchmarkBatch,
        identity: &str,
        now: Timestamp,
    ) -> Result<bool> {
        let new_run = self.insert_run(&b.run, identity)?;
        self.upsert_environment(&b.environment, now)?;
        for bm in &b.benchmarks {
            self.upsert_benchmark(bm)?;
        }
        if new_run {
            for br in &b.benchmark_runs {
                self.insert_benchmark_run(br)?;
            }
        }
        Ok(new_run)
    }
}
