use crate::ids::{
    AdrId, BenchmarkId, CommitKey, ComponentId, FeatureId, SuiteId, TestId, VisualCheckId,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::Low => "low",
        }
    }
    pub fn parse(s: &str) -> Confidence {
        match s {
            "high" => Confidence::High,
            "medium" => Confidence::Medium,
            _ => Confidence::Low,
        }
    }
    pub fn min(self, other: Confidence) -> Confidence {
        // Ord: High < Medium < Low, so the "lower" confidence is the max.
        if other > self { other } else { self }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum EntityRef {
    Feature(FeatureId),
    Adr(AdrId),
    Test(TestId),
    Benchmark(BenchmarkId),
    Visual(VisualCheckId),
    Commit(CommitKey),
    Component(ComponentId),
    Suite(SuiteId),
}

impl EntityRef {
    pub fn kind_str(&self) -> &'static str {
        match self {
            EntityRef::Feature(_) => "feature",
            EntityRef::Adr(_) => "adr",
            EntityRef::Test(_) => "test",
            EntityRef::Benchmark(_) => "benchmark",
            EntityRef::Visual(_) => "visual",
            EntityRef::Commit(_) => "commit",
            EntityRef::Component(_) => "component",
            EntityRef::Suite(_) => "suite",
        }
    }
    pub fn id_str(&self) -> String {
        match self {
            EntityRef::Feature(i) => i.0.clone(),
            EntityRef::Adr(i) => i.0.clone(),
            EntityRef::Test(i) => i.0.clone(),
            EntityRef::Benchmark(i) => i.0.clone(),
            EntityRef::Visual(i) => i.0.clone(),
            EntityRef::Commit(k) => format!("{}@{}", k.repo, k.sha),
            EntityRef::Component(i) => i.0.clone(),
            EntityRef::Suite(i) => i.0.clone(),
        }
    }
    pub fn parse(kind: &str, id: &str) -> Option<EntityRef> {
        Some(match kind {
            "feature" => EntityRef::Feature(FeatureId::new(id)),
            "adr" => EntityRef::Adr(AdrId::new(id)),
            "test" => EntityRef::Test(TestId::new(id)),
            "benchmark" => EntityRef::Benchmark(BenchmarkId::new(id)),
            "visual" => EntityRef::Visual(VisualCheckId::new(id)),
            "commit" => {
                let (repo, sha) = id.split_once('@')?;
                EntityRef::Commit(CommitKey { repo: repo.into(), sha: sha.into() })
            }
            "component" => EntityRef::Component(ComponentId::new(id)),
            "suite" => EntityRef::Suite(SuiteId::new(id)),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    TestCovers,
    BenchmarkMeasures,
    VisualShows,
    CommitAffects,
    AdrGoverns,
    AdrSupersedes,
    FeatureDependsOn,
    FeatureSupersedes,
    ClaimsComponent,
    SuiteFeeds,
}

impl RelationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RelationKind::TestCovers => "test_covers",
            RelationKind::BenchmarkMeasures => "benchmark_measures",
            RelationKind::VisualShows => "visual_shows",
            RelationKind::CommitAffects => "commit_affects",
            RelationKind::AdrGoverns => "adr_governs",
            RelationKind::AdrSupersedes => "adr_supersedes",
            RelationKind::FeatureDependsOn => "feature_depends_on",
            RelationKind::FeatureSupersedes => "feature_supersedes",
            RelationKind::ClaimsComponent => "claims_component",
            RelationKind::SuiteFeeds => "suite_feeds",
        }
    }
    pub fn parse(s: &str) -> Option<RelationKind> {
        Some(match s {
            "test_covers" => RelationKind::TestCovers,
            "benchmark_measures" => RelationKind::BenchmarkMeasures,
            "visual_shows" => RelationKind::VisualShows,
            "commit_affects" => RelationKind::CommitAffects,
            "adr_governs" => RelationKind::AdrGoverns,
            "adr_supersedes" => RelationKind::AdrSupersedes,
            "feature_depends_on" => RelationKind::FeatureDependsOn,
            "feature_supersedes" => RelationKind::FeatureSupersedes,
            "claims_component" => RelationKind::ClaimsComponent,
            "suite_feeds" => RelationKind::SuiteFeeds,
            _ => return None,
        })
    }
}

/// How a relationship was established. Explicit relationships come from something the author
/// wrote; inferred ones from a heuristic, and they say which one.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "origin", content = "source", rename_all = "snake_case")]
pub enum RelationOrigin {
    Explicit(ExplicitSource),
    Inferred(InferredSource),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplicitSource {
    Frontmatter,
    TestTag,
    TestName,
    Interchange,
    TestMap,
    CommitMessage,
    AdrBody,
    Config,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferredSource {
    PathMatch { glob: String },
    AdrMention,
    TimestampCommit,
    ComponentDefault,
}

impl RelationOrigin {
    pub fn is_explicit(&self) -> bool {
        matches!(self, RelationOrigin::Explicit(_))
    }
    /// A short stable key for storage/dedup (`explicit:test_tag`, `inferred:path_match`).
    pub fn key(&self) -> String {
        match self {
            RelationOrigin::Explicit(s) => {
                format!("explicit:{}", serde_json::to_value(s).unwrap().as_str().unwrap_or("?"))
            }
            RelationOrigin::Inferred(InferredSource::PathMatch { .. }) => {
                "inferred:path_match".into()
            }
            RelationOrigin::Inferred(s) => {
                format!("inferred:{}", serde_json::to_value(s).unwrap().as_str().unwrap_or("?"))
            }
        }
    }
    pub fn note(&self) -> String {
        match self {
            RelationOrigin::Explicit(ExplicitSource::Frontmatter) => {
                "declared in frontmatter".into()
            }
            RelationOrigin::Explicit(ExplicitSource::TestTag) => "test tag".into(),
            RelationOrigin::Explicit(ExplicitSource::TestName) => "feature id in test name".into(),
            RelationOrigin::Explicit(ExplicitSource::Interchange) => {
                "declared by the result document".into()
            }
            RelationOrigin::Explicit(ExplicitSource::TestMap) => "test map entry".into(),
            RelationOrigin::Explicit(ExplicitSource::CommitMessage) => {
                "feature id in commit message".into()
            }
            RelationOrigin::Explicit(ExplicitSource::AdrBody) => "referenced in ADR".into(),
            RelationOrigin::Explicit(ExplicitSource::Config) => "configured".into(),
            RelationOrigin::Inferred(InferredSource::PathMatch { glob }) => {
                format!("path match {glob}")
            }
            RelationOrigin::Inferred(InferredSource::AdrMention) => "mentioned in ADR body".into(),
            RelationOrigin::Inferred(InferredSource::TimestampCommit) => {
                "inferred from timestamps".into()
            }
            RelationOrigin::Inferred(InferredSource::ComponentDefault) => {
                "default component of the suite".into()
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Relationship {
    pub from: EntityRef,
    pub to: EntityRef,
    pub kind: RelationKind,
    pub origin: RelationOrigin,
    pub confidence: Confidence,
}

/// A reference that could not be resolved (feature → missing ADR, test → unknown feature …).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnresolvedRef {
    pub from: EntityRef,
    pub raw: String,
    pub target_kind: String,
    pub note: Option<String>,
}
