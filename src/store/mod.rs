//! The derived SQLite index (`.cockpit/index.sqlite`). Entities are stored as JSON documents
//! with a few indexed columns beside them; the JSON is the model, the columns exist for
//! queries. Deleting the file is always safe.

pub mod read;
pub mod write;

use crate::time::millis;
use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: i64 = 1;
const SCHEMA_SQL: &str = include_str!("schema.sql");

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub struct Store {
    pub(crate) conn: Connection,
    pub path: PathBuf,
}

impl Store {
    /// Open (creating directories and schema as needed). A schema-version mismatch drops and
    /// recreates the database: the index is a cache by design.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
            let gi = dir.join(".gitignore");
            if !gi.exists() {
                let _ = std::fs::write(&gi, "*\n");
            }
        }
        let mut store = Store { conn: Connection::open(path)?, path: path.to_path_buf() };
        store.configure()?;
        store.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        let existing: Option<i64> = store.meta("schema_version")?.and_then(|v| v.parse().ok());
        match existing {
            Some(v) if v == SCHEMA_VERSION => {}
            Some(_) => {
                tracing::warn!("index schema changed; rebuilding {}", path.display());
                drop(store);
                let _ = std::fs::remove_file(path);
                let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
                let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
                store = Store { conn: Connection::open(path)?, path: path.to_path_buf() };
                store.configure()?;
                store.migrate()?;
            }
            None => store.migrate()?,
        }
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Store> {
        let mut store =
            Store { conn: Connection::open_in_memory()?, path: PathBuf::from(":memory:") };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    fn configure(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;",
        )?;
        Ok(())
    }

    fn migrate(&mut self) -> Result<()> {
        self.conn.execute_batch(SCHEMA_SQL)?;
        self.set_meta("schema_version", &SCHEMA_VERSION.to_string())?;
        self.set_meta("cockpit_version", crate::VERSION)?;
        Ok(())
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn meta_timestamp(&self, key: &str) -> Result<Option<Timestamp>> {
        Ok(self.meta(key)?.and_then(|v| v.parse::<i64>().ok()).map(crate::time::from_millis))
    }

    pub fn set_meta_timestamp(&self, key: &str, t: Timestamp) -> Result<()> {
        self.set_meta(key, &millis(t).to_string())
    }

    /// Wipe every rebuildable table (history is kept) — used by `scan --full`.
    pub fn clear_cache(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM scan_files; DELETE FROM features; DELETE FROM adrs; DELETE FROM commits; DELETE FROM commit_files;
             DELETE FROM relationships; DELETE FROM unresolved_refs; DELETE FROM feature_health; DELETE FROM warnings;
             DELETE FROM artifacts;",
        )?;
        Ok(())
    }

    /// Wipe history too — `scan --full --reset-history`.
    pub fn clear_all(&self) -> Result<()> {
        self.clear_cache()?;
        self.conn.execute_batch(
            "DELETE FROM test_runs; DELETE FROM benchmark_runs; DELETE FROM visual_evidence; DELETE FROM runs;
             DELETE FROM tests; DELETE FROM suites; DELETE FROM benchmarks; DELETE FROM environments;
             DELETE FROM attention_items; DELETE FROM events; DELETE FROM repos;",
        )?;
        Ok(())
    }

    /// Coarse batching: many small upserts inside one transaction.
    pub fn batch_begin(&self) -> Result<()> {
        self.conn.execute_batch("BEGIN")?;
        Ok(())
    }

    pub fn batch_commit(&self) -> Result<()> {
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }

    pub fn count(&self, table: &str) -> Result<i64> {
        let sql = format!("SELECT count(*) FROM {table}");
        Ok(self.conn.query_row(&sql, [], |r| r.get(0))?)
    }

    pub fn integrity_ok(&self) -> Result<bool> {
        let s: String = self.conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        Ok(s == "ok")
    }

    pub fn size_bytes(&self) -> u64 {
        std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_and_versions() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(".cockpit").join("index.sqlite");
        let s = Store::open(&p).unwrap();
        assert_eq!(s.meta("schema_version").unwrap().as_deref(), Some("1"));
        assert!(tmp.path().join(".cockpit/.gitignore").exists());
        assert!(s.integrity_ok().unwrap());
        assert_eq!(s.count("features").unwrap(), 0);
    }
}
