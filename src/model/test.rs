use crate::ids::{ArtifactId, ComponentId, RepoId, RunId, SourceId, SuiteId, TestId, TestRunId};
use crate::model::git::CommitRef;
use crate::model::provenance::Provenance;
use crate::model::visual::Viewport;
use crate::paths::RelPath;
use crate::time::{Origin, Stamped};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestKind {
    Unit,
    Integration,
    E2e,
    Contract,
    Visual,
    Smoke,
    Security,
    Conformance,
    Perf,
    Unknown,
}

impl TestKind {
    pub fn parse(s: &str) -> TestKind {
        match s.trim().to_ascii_lowercase().as_str() {
            "unit" => TestKind::Unit,
            "integration" | "int" => TestKind::Integration,
            "e2e" | "end-to-end" | "browser" => TestKind::E2e,
            "contract" => TestKind::Contract,
            "visual" | "screenshot" => TestKind::Visual,
            "smoke" => TestKind::Smoke,
            "security" => TestKind::Security,
            "conformance" | "corpus" => TestKind::Conformance,
            "perf" | "performance" | "benchmark" => TestKind::Perf,
            _ => TestKind::Unknown,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            TestKind::Unit => "unit",
            TestKind::Integration => "integration",
            TestKind::E2e => "e2e",
            TestKind::Contract => "contract",
            TestKind::Visual => "visual",
            TestKind::Smoke => "smoke",
            TestKind::Security => "security",
            TestKind::Conformance => "conformance",
            TestKind::Perf => "perf",
            TestKind::Unknown => "unknown",
        }
    }
    /// The evidence category a kind rolls up into on the feature page.
    pub fn category(&self) -> EvidenceCategory {
        match self {
            TestKind::Unit => EvidenceCategory::Unit,
            TestKind::Integration
            | TestKind::Contract
            | TestKind::Security
            | TestKind::Conformance => EvidenceCategory::Integration,
            TestKind::E2e | TestKind::Smoke => EvidenceCategory::E2e,
            TestKind::Visual => EvidenceCategory::Visual,
            TestKind::Perf => EvidenceCategory::Perf,
            TestKind::Unknown => EvidenceCategory::Unit,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceCategory {
    Unit,
    Integration,
    E2e,
    Visual,
    Perf,
    Coverage,
}

impl EvidenceCategory {
    pub const ALL: [EvidenceCategory; 5] = [
        EvidenceCategory::Unit,
        EvidenceCategory::Integration,
        EvidenceCategory::E2e,
        EvidenceCategory::Visual,
        EvidenceCategory::Perf,
    ];
    pub fn as_str(&self) -> &'static str {
        match self {
            EvidenceCategory::Unit => "unit",
            EvidenceCategory::Integration => "integration",
            EvidenceCategory::E2e => "e2e",
            EvidenceCategory::Visual => "visual",
            EvidenceCategory::Perf => "performance",
            EvidenceCategory::Coverage => "coverage",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            EvidenceCategory::Unit => "Unit",
            EvidenceCategory::Integration => "Integration",
            EvidenceCategory::E2e => "E2E",
            EvidenceCategory::Visual => "Visual",
            EvidenceCategory::Perf => "Perf",
            EvidenceCategory::Coverage => "Coverage",
        }
    }
    pub fn parse(s: &str) -> Option<EvidenceCategory> {
        Some(match s {
            "unit" => EvidenceCategory::Unit,
            "integration" => EvidenceCategory::Integration,
            "e2e" => EvidenceCategory::E2e,
            "visual" => EvidenceCategory::Visual,
            "performance" | "perf" => EvidenceCategory::Perf,
            "coverage" => EvidenceCategory::Coverage,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
    /// Passed only on retry.
    Flaky,
}

impl TestStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TestStatus::Passed => "passed",
            TestStatus::Failed => "failed",
            TestStatus::Skipped => "skipped",
            TestStatus::Flaky => "flaky",
        }
    }
    pub fn parse(s: &str) -> Option<TestStatus> {
        Some(match s {
            "passed" => TestStatus::Passed,
            "failed" => TestStatus::Failed,
            "skipped" => TestStatus::Skipped,
            "flaky" => TestStatus::Flaky,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunCounts {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub flaky: u32,
}

impl RunCounts {
    pub fn total(&self) -> u32 {
        self.passed + self.failed + self.skipped + self.flaky
    }
    pub fn add(&mut self, s: TestStatus) {
        match s {
            TestStatus::Passed => self.passed += 1,
            TestStatus::Failed => self.failed += 1,
            TestStatus::Skipped => self.skipped += 1,
            TestStatus::Flaky => self.flaky += 1,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Suite {
    pub id: SuiteId,
    pub source: SourceId,
    pub name: String,
    pub default_kind: TestKind,
    pub component: Option<ComponentId>,
    pub repo: Option<RepoId>,
    pub expect_every: Option<SignedDuration>,
    pub last_seen: Option<Timestamp>,
    pub last_run: Option<RunId>,
    pub run_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Test {
    pub id: TestId,
    pub suite: SuiteId,
    /// The stable per-suite identity (`classname::name`, `file::title::project`, …).
    pub key: String,
    pub name: String,
    pub classname: Option<String>,
    pub kind: TestKind,
    pub source_path: Option<RelPath>,
    pub source_path_origin: Option<Origin>,
    pub line: Option<u32>,
    pub tags: Vec<String>,
    pub component: Option<ComponentId>,
    /// Feature ids declared by the test itself (tags, name suffixes, interchange `features[]`).
    pub declared_feature_ids: Vec<String>,
    pub level: Option<String>,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
}

/// One imported result document (a JUnit file, a Playwright report, a bench record …).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub id: RunId,
    pub source: SourceId,
    pub suite: Option<SuiteId>,
    pub kind: RunKind,
    pub provenance: Provenance,
    pub finished_at: Stamped<Timestamp>,
    pub started_at: Option<Timestamp>,
    pub counts: RunCounts,
    /// No usable results (all skipped, empty, or the inputs were missing) — never green, never red.
    pub silent: bool,
    pub silent_reason: Option<String>,
    /// What makes this run unique within its source (suite + declared run id or content hash).
    #[serde(default)]
    pub identity: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunKind {
    Test,
    Benchmark,
    Visual,
}

impl RunKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunKind::Test => "test",
            RunKind::Benchmark => "benchmark",
            RunKind::Visual => "visual",
        }
    }
    pub fn parse(s: &str) -> RunKind {
        match s {
            "benchmark" => RunKind::Benchmark,
            "visual" => RunKind::Visual,
            _ => RunKind::Test,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TestRun {
    pub id: TestRunId,
    pub run: RunId,
    pub test: TestId,
    pub status: TestStatus,
    pub duration_ms: Option<u64>,
    pub retries: u32,
    pub timestamp: Stamped<Timestamp>,
    pub message: Option<String>,
    pub details: Option<String>,
    pub artifacts: Vec<ArtifactId>,
    pub browser: Option<String>,
    pub viewport: Option<Viewport>,
    pub commit: Option<CommitRef>,
}
