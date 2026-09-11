-- Cockpit derived index. Deletable: everything here is rebuilt from the repository, except
-- the HISTORY tables (runs, test_runs, benchmark_runs, measurements, visual_evidence,
-- attention_items.first_seen, events, environments) which accumulate across scans.

CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE TABLE IF NOT EXISTS scan_files (
  path TEXT PRIMARY KEY,
  mtime_ms INTEGER NOT NULL,
  size INTEGER NOT NULL,
  content_hash TEXT NOT NULL,
  importer TEXT NOT NULL,
  importer_version INTEGER NOT NULL,
  source_id TEXT,
  last_scanned_at INTEGER NOT NULL,
  status TEXT NOT NULL,
  warnings_json TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX IF NOT EXISTS scan_files_importer ON scan_files(importer);

CREATE TABLE IF NOT EXISTS repos (
  id TEXT PRIMARY KEY,
  path TEXT NOT NULL,
  primary_branch TEXT,
  head_sha TEXT,
  head_branch TEXT,
  dirty INTEGER NOT NULL DEFAULT 0,
  dirty_files INTEGER NOT NULL DEFAULT 0,
  commit_count INTEGER NOT NULL DEFAULT 0,
  last_synced_at INTEGER,
  note TEXT
);

CREATE TABLE IF NOT EXISTS features (
  id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  title TEXT NOT NULL,
  lifecycle TEXT NOT NULL,
  importance TEXT NOT NULL,
  source_path TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  first_seen INTEGER NOT NULL,
  last_seen INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS features_source ON features(source_path);

CREATE TABLE IF NOT EXISTS adrs (
  id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  number INTEGER,
  title TEXT NOT NULL,
  status TEXT NOT NULL,
  source_path TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  first_seen INTEGER NOT NULL,
  last_seen INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS adrs_number ON adrs(number);

CREATE TABLE IF NOT EXISTS suites (
  id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  source_id TEXT NOT NULL,
  last_seen INTEGER,
  run_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS tests (
  id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  suite_id TEXT NOT NULL,
  test_key TEXT NOT NULL,
  source_path TEXT,
  first_seen INTEGER NOT NULL,
  last_seen INTEGER NOT NULL,
  UNIQUE (suite_id, test_key)
);
CREATE INDEX IF NOT EXISTS tests_source_path ON tests(source_path);

-- HISTORY: one row per imported result document.
CREATE TABLE IF NOT EXISTS runs (
  id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  source_id TEXT NOT NULL,
  suite_id TEXT,
  kind TEXT NOT NULL,
  identity TEXT NOT NULL,
  repo TEXT,
  commit_sha TEXT,
  branch TEXT,
  finished_at INTEGER NOT NULL,
  source_path TEXT NOT NULL,
  silent INTEGER NOT NULL DEFAULT 0,
  imported_at INTEGER NOT NULL,
  UNIQUE (source_id, identity)
);
CREATE INDEX IF NOT EXISTS runs_suite_time ON runs(suite_id, finished_at DESC);
CREATE INDEX IF NOT EXISTS runs_commit ON runs(repo, commit_sha);

-- HISTORY
CREATE TABLE IF NOT EXISTS test_runs (
  id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  test_id TEXT NOT NULL,
  status TEXT NOT NULL,
  duration_ms INTEGER,
  timestamp INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS test_runs_test_time ON test_runs(test_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS test_runs_run ON test_runs(run_id);

CREATE TABLE IF NOT EXISTS environments (
  id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  class_id TEXT NOT NULL,
  first_seen INTEGER NOT NULL,
  last_seen INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS benchmarks (
  id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  source_id TEXT NOT NULL,
  name TEXT NOT NULL,
  first_seen INTEGER NOT NULL,
  last_seen INTEGER NOT NULL
);

-- HISTORY
CREATE TABLE IF NOT EXISTS benchmark_runs (
  id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  benchmark_id TEXT NOT NULL,
  env_class_id TEXT NOT NULL,
  value REAL NOT NULL,
  timestamp INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS bench_runs_series ON benchmark_runs(benchmark_id, env_class_id, timestamp DESC);

-- HISTORY
CREATE TABLE IF NOT EXISTS visual_evidence (
  id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  check_id TEXT NOT NULL,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  status TEXT NOT NULL,
  timestamp INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS visual_check_time ON visual_evidence(check_id, timestamp DESC);

CREATE TABLE IF NOT EXISTS artifacts (
  id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  last_seen INTEGER NOT NULL
);

-- CACHE of git history (rebuildable).
CREATE TABLE IF NOT EXISTS commits (
  repo TEXT NOT NULL,
  sha TEXT NOT NULL,
  data_json TEXT NOT NULL,
  committed_at INTEGER NOT NULL,
  subject TEXT NOT NULL,
  PRIMARY KEY (repo, sha)
);
CREATE INDEX IF NOT EXISTS commits_time ON commits(repo, committed_at DESC);

CREATE TABLE IF NOT EXISTS commit_files (
  repo TEXT NOT NULL,
  sha TEXT NOT NULL,
  path TEXT NOT NULL,
  PRIMARY KEY (repo, sha, path)
);
CREATE INDEX IF NOT EXISTS commit_files_path ON commit_files(path);

CREATE TABLE IF NOT EXISTS relationships (
  from_kind TEXT NOT NULL, from_id TEXT NOT NULL,
  to_kind TEXT NOT NULL, to_id TEXT NOT NULL,
  rel TEXT NOT NULL, origin_key TEXT NOT NULL,
  data_json TEXT NOT NULL,
  PRIMARY KEY (from_kind, from_id, to_kind, to_id, rel, origin_key)
);
CREATE INDEX IF NOT EXISTS rel_to ON relationships(to_kind, to_id, rel);
CREATE INDEX IF NOT EXISTS rel_from ON relationships(from_kind, from_id, rel);

CREATE TABLE IF NOT EXISTS unresolved_refs (
  from_kind TEXT NOT NULL, from_id TEXT NOT NULL, raw TEXT NOT NULL,
  data_json TEXT NOT NULL,
  PRIMARY KEY (from_kind, from_id, raw)
);

CREATE TABLE IF NOT EXISTS feature_health (
  feature_id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  state TEXT NOT NULL,
  evaluated_at INTEGER NOT NULL
);

-- HISTORY (first_seen survives)
CREATE TABLE IF NOT EXISTS attention_items (
  id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  status TEXT NOT NULL,
  feature_id TEXT,
  score INTEGER NOT NULL,
  first_seen INTEGER NOT NULL,
  last_seen INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS attention_open ON attention_items(status, score DESC);

-- HISTORY
CREATE TABLE IF NOT EXISTS events (
  id TEXT PRIMARY KEY,
  data_json TEXT NOT NULL,
  kind TEXT NOT NULL,
  at INTEGER NOT NULL,
  feature_id TEXT
);
CREATE INDEX IF NOT EXISTS events_at ON events(at DESC);

CREATE TABLE IF NOT EXISTS warnings (
  path TEXT NOT NULL,
  importer TEXT NOT NULL,
  message TEXT NOT NULL,
  location TEXT,
  fatal INTEGER NOT NULL,
  seen_at INTEGER NOT NULL
);
