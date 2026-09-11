//! Importer adapters: every external source of engineering state enters through one of these.
//! An importer never fails hard — malformed input becomes warnings plus whatever partial
//! entities could be recovered (§121: a malformed file must not take the project down).

pub mod adr_md;
pub mod batch;
pub mod benchmark_json;
pub mod cargo_index;
pub mod criterion;
pub mod feature_md;
pub mod frontmatter;
pub mod junit;
pub mod markdown;
pub mod playwright;
pub mod provenance;
pub mod results_json;
pub mod sniff;

use crate::config::{EvidenceFormat, EvidenceSource, Located};
use crate::hash::ContentHash;
use crate::ids::{FeatureId, RepoId, SourceId};
use crate::model::*;
use crate::paths::RelPath;
use jiff::Timestamp;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImporterKind {
    FeatureMd,
    AdrMd,
    Evidence(EvidenceFormat),
}

impl ImporterKind {
    pub fn as_str(&self) -> String {
        match self {
            ImporterKind::FeatureMd => "feature-md".into(),
            ImporterKind::AdrMd => "adr-md".into(),
            ImporterKind::Evidence(f) => f.as_str().into(),
        }
    }
}

/// Everything an importer may need to know about the project while reading one file.
pub struct ImportContext<'a> {
    pub located: &'a Located,
    pub project: &'a Project,
    /// Repo path prefix → repo id, longest prefix first.
    pub repos: &'a RepoIndex,
    /// Known feature ids (empty during the first pass of a scan; ADRs and evidence see it).
    pub feature_ids: &'a HashSet<FeatureId>,
    pub source: Option<&'a EvidenceSource>,
    pub source_id: Option<SourceId>,
    pub now: Timestamp,
    pub cargo: &'a cargo_index::CargoWorkspaceIndex,
    pub sidecars: &'a provenance::SidecarIndex,
    pub commits: &'a provenance::CommitLocator,
}

#[derive(Debug, Default, Clone)]
pub struct RepoIndex {
    /// (prefix, id), sorted longest prefix first; the root repo has an empty prefix.
    entries: Vec<(RelPath, RepoId)>,
}

impl RepoIndex {
    pub fn new(repos: &[Repo]) -> Self {
        let mut entries: Vec<(RelPath, RepoId)> =
            repos.iter().map(|r| (r.path.clone(), r.id.clone())).collect();
        entries.sort_by_key(|(p, _)| std::cmp::Reverse(p.as_str().len()));
        RepoIndex { entries }
    }
    pub fn repo_for(&self, path: &RelPath) -> Option<&RepoId> {
        self.entries.iter().find(|(p, _)| path.starts_with(p)).map(|(_, id)| id)
    }
    pub fn prefix_of(&self, id: &RepoId) -> Option<&RelPath> {
        self.entries.iter().find(|(_, i)| i == id).map(|(p, _)| p)
    }

    /// Accepts a repo id (`plugin-doc`) or its project-relative path (`plugins/plugin-doc`).
    pub fn resolve(&self, id_or_path: &str) -> Option<RepoId> {
        let p = RelPath::new(id_or_path);
        self.entries
            .iter()
            .find(|(path, id)| {
                id.as_str() == id_or_path
                    || (!path.is_root() && *path == p)
                    || (path.is_root() && id_or_path == ".")
            })
            .map(|(_, id)| id.clone())
    }
}

#[derive(Debug, Clone)]
pub struct ImportInput {
    pub abs: PathBuf,
    pub rel: RelPath,
    pub bytes: Vec<u8>,
    pub content_hash: ContentHash,
    pub mtime: Timestamp,
    pub size: u64,
}

impl ImportInput {
    pub fn text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.bytes)
    }
}

#[derive(Debug, Default)]
pub struct ImportOutput {
    pub entities: Vec<ImportedEntity>,
    pub warnings: Vec<ImportWarning>,
}

impl ImportOutput {
    pub fn warn(
        &mut self,
        path: &RelPath,
        importer: &str,
        message: impl Into<String>,
        location: Option<String>,
    ) {
        self.warnings.push(ImportWarning {
            path: path.clone(),
            importer: importer.to_string(),
            message: message.into(),
            location,
            fatal: false,
        });
    }
    pub fn fatal(
        &mut self,
        path: &RelPath,
        importer: &str,
        message: impl Into<String>,
        location: Option<String>,
    ) {
        self.warnings.push(ImportWarning {
            path: path.clone(),
            importer: importer.to_string(),
            message: message.into(),
            location,
            fatal: true,
        });
    }
}

#[derive(Debug)]
pub enum ImportedEntity {
    Feature(Feature),
    Adr(Adr),
    TestBatch(TestBatch),
    BenchmarkBatch(BenchmarkBatch),
}

#[derive(Debug)]
pub struct TestBatch {
    pub suite: Suite,
    pub run: Run,
    pub tests: Vec<Test>,
    pub test_runs: Vec<TestRun>,
    pub artifacts: Vec<Artifact>,
    pub visuals: Vec<VisualEvidence>,
}

#[derive(Debug)]
pub struct BenchmarkBatch {
    pub run: Run,
    pub environment: EnvironmentFingerprint,
    pub benchmarks: Vec<Benchmark>,
    pub benchmark_runs: Vec<BenchmarkRun>,
}

pub trait Importer: Send + Sync {
    fn kind(&self) -> ImporterKind;
    /// Bump to force re-import of files previously read by an older version.
    fn version(&self) -> u32;
    fn import(&self, ctx: &ImportContext, input: &ImportInput) -> ImportOutput;
}

/// All importers Cockpit ships, keyed by kind.
pub fn registry() -> BTreeMap<String, Box<dyn Importer>> {
    let mut m: BTreeMap<String, Box<dyn Importer>> = BTreeMap::new();
    for imp in [
        Box::new(feature_md::FeatureMdImporter) as Box<dyn Importer>,
        Box::new(adr_md::AdrMdImporter),
    ] {
        m.insert(imp.kind().as_str(), imp);
    }
    m
}

pub fn importer_for(kind: ImporterKind) -> Option<Box<dyn Importer>> {
    match kind {
        ImporterKind::FeatureMd => Some(Box::new(feature_md::FeatureMdImporter)),
        ImporterKind::AdrMd => Some(Box::new(adr_md::AdrMdImporter)),
        ImporterKind::Evidence(EvidenceFormat::Junit) => Some(Box::new(junit::JunitImporter)),
        ImporterKind::Evidence(EvidenceFormat::Playwright) => {
            Some(Box::new(playwright::PlaywrightImporter))
        }
        ImporterKind::Evidence(
            f @ (EvidenceFormat::CockpitResults | EvidenceFormat::PagedResults),
        ) => Some(Box::new(results_json::ResultsJsonImporter { format: f })),
        ImporterKind::Evidence(EvidenceFormat::CockpitBenchmark) => {
            Some(Box::new(benchmark_json::BenchmarkJsonImporter))
        }
        ImporterKind::Evidence(EvidenceFormat::GuardianBench) => {
            Some(Box::new(benchmark_json::GuardianBenchImporter))
        }
        ImporterKind::Evidence(EvidenceFormat::PagedMeasurements) => {
            Some(Box::new(benchmark_json::PagedMeasurementsImporter))
        }
        ImporterKind::Evidence(EvidenceFormat::Criterion) => {
            Some(Box::new(criterion::CriterionImporter))
        }
        ImporterKind::Evidence(_) => None,
    }
}

/// Convenience for frontmatter values that may be a scalar or a list.
pub fn yaml_string_list(v: Option<&serde_yaml_ng::Value>) -> Vec<String> {
    match v {
        None | Some(serde_yaml_ng::Value::Null) => vec![],
        Some(serde_yaml_ng::Value::Sequence(seq)) => {
            seq.iter().filter_map(yaml_scalar_string).collect()
        }
        Some(other) => yaml_scalar_string(other).into_iter().collect(),
    }
}

pub fn yaml_scalar_string(v: &serde_yaml_ng::Value) -> Option<String> {
    match v {
        serde_yaml_ng::Value::String(s) => Some(s.clone()),
        serde_yaml_ng::Value::Number(n) => Some(n.to_string()),
        serde_yaml_ng::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub fn yaml_to_json(v: &serde_yaml_ng::Value) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
}
