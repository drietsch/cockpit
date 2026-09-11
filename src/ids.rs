//! Stable identifiers. Every long-lived engineering concept gets a newtype so that ids of
//! different kinds cannot be mixed up, and derived ids are computed in exactly one place.

use crate::hash::short_id;
use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(s: impl Into<String>) -> Self {
                $name(s.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                $name(s.to_string())
            }
        }
        impl From<String> for $name {
            fn from(s: String) -> Self {
                $name(s)
            }
        }
        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

string_id!(
    /// Opaque feature id as written by the project (`FEAT-019`, `layers.ops`).
    FeatureId
);
string_id!(
    /// Canonical ADR id, `ADR-<digits as written>` (`ADR-0057`, `ADR-025`).
    AdrId
);
string_id!(
    /// Repository id from `[[repos]]` (default: directory name; the root repo is the project id).
    RepoId
);
string_id!(
    /// Full 40-hex commit sha (lowercase).
    Sha
);
string_id!(
    /// Evidence source id from `[[evidence]]` (`core/nextest`, or `auto:junit`).
    SourceId
);
string_id!(
    /// Suite id, `<source id>` or `<source id>#<suite name>` when a source yields several suites.
    SuiteId
);
string_id!(TestId);
string_id!(RunId);
string_id!(TestRunId);
string_id!(BenchmarkId);
string_id!(BenchmarkRunId);
string_id!(VisualCheckId);
string_id!(VisualEvidenceId);
string_id!(ArtifactId);
string_id!(EnvironmentId);
string_id!(EnvClassId);
string_id!(AttentionId);
string_id!(EventId);
string_id!(ComponentId);

impl Sha {
    pub fn short(&self) -> &str {
        &self.0[..self.0.len().min(7)]
    }
}

/// A commit is only unique within its repository.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CommitKey {
    pub repo: RepoId,
    pub sha: Sha,
}

impl std::fmt::Debug for CommitKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.repo, self.sha.short())
    }
}

impl std::fmt::Display for CommitKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.repo, self.sha.short())
    }
}

impl TestId {
    pub fn derive(suite: &SuiteId, test_key: &str) -> Self {
        TestId(format!("T-{}", short_id("test", &[suite.as_str(), test_key])))
    }
}
impl RunId {
    pub fn derive(source: &SourceId, identity: &str) -> Self {
        RunId(format!("R-{}", short_id("run", &[source.as_str(), identity])))
    }
}
impl TestRunId {
    pub fn derive(run: &RunId, test: &TestId) -> Self {
        TestRunId(format!("TR-{}", short_id("test_run", &[run.as_str(), test.as_str()])))
    }
}
impl BenchmarkId {
    pub fn derive(source: &SourceId, name: &str, subject: Option<&str>, metric: &str) -> Self {
        BenchmarkId(format!(
            "B-{}",
            short_id("bench", &[source.as_str(), name, subject.unwrap_or(""), metric])
        ))
    }
}
impl BenchmarkRunId {
    pub fn derive(run: &RunId, bench: &BenchmarkId) -> Self {
        BenchmarkRunId(format!("BR-{}", short_id("bench_run", &[run.as_str(), bench.as_str()])))
    }
}
impl VisualCheckId {
    pub fn derive(suite: &SuiteId, subject: &str, snapshot: &str) -> Self {
        VisualCheckId(format!("V-{}", short_id("visual", &[suite.as_str(), subject, snapshot])))
    }
}
impl VisualEvidenceId {
    pub fn derive(check: &VisualCheckId, run: &RunId) -> Self {
        VisualEvidenceId(format!("VE-{}", short_id("visual_ev", &[check.as_str(), run.as_str()])))
    }
}
impl ArtifactId {
    pub fn derive(resolved_path: &str) -> Self {
        ArtifactId(format!("AR-{}", short_id("artifact", &[resolved_path])))
    }
}
impl EnvironmentId {
    pub fn derive(canonical_json: &str) -> Self {
        EnvironmentId(format!("E-{}", short_id("env", &[canonical_json])))
    }
}
impl EnvClassId {
    pub fn derive(canonical_json: &str) -> Self {
        EnvClassId(format!("EC-{}", short_id("envclass", &[canonical_json])))
    }
}
impl AttentionId {
    pub fn derive(reason: &str, feature: Option<&FeatureId>, subject: &str) -> Self {
        AttentionId(format!(
            "A-{}",
            short_id("attention", &[reason, feature.map(|f| f.as_str()).unwrap_or(""), subject])
        ))
    }
}
impl EventId {
    pub fn derive(kind: &str, feature: Option<&FeatureId>, subject: &str, at_minute: i64) -> Self {
        EventId(format!(
            "EV-{}",
            short_id(
                "event",
                &[kind, feature.map(|f| f.as_str()).unwrap_or(""), subject, &at_minute.to_string()]
            )
        ))
    }
}

impl AdrId {
    /// Normalise any of `ADR-0057`, `ADR 057`, `adr-57`, `0057`, `57` into the canonical
    /// `ADR-<digits as written>` form. Returns `None` if no digits are present.
    pub fn parse_ref(raw: &str) -> Option<AdrId> {
        let t = raw.trim().trim_matches(|c: char| c == '[' || c == ']' || c == '`');
        let digits: String = {
            let lower = t.to_ascii_lowercase();
            let rest = lower.strip_prefix("adr").unwrap_or(&lower);
            let rest = rest.trim_start_matches(['-', ' ', '_']);
            rest.chars().take_while(|c| c.is_ascii_digit()).collect()
        };
        if digits.is_empty() {
            return None;
        }
        Some(AdrId(format!("ADR-{digits}")))
    }

    /// The numeric part, for cross-format resolution (`ADR-0057` and `ADR-57` both → 57).
    pub fn number(&self) -> Option<u32> {
        self.0.strip_prefix("ADR-")?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adr_refs_normalise() {
        for (raw, want) in [
            ("ADR-0057", "ADR-0057"),
            ("ADR 025", "ADR-025"),
            ("adr-7", "ADR-7"),
            ("[ADR-0016]", "ADR-0016"),
            ("0022", "ADR-0022"),
        ] {
            assert_eq!(AdrId::parse_ref(raw).unwrap().as_str(), want, "{raw}");
        }
        assert!(AdrId::parse_ref("ADR-XXXX").is_none());
        assert_eq!(AdrId::new("ADR-0057").number(), Some(57));
    }

    #[test]
    fn derived_ids_are_deterministic() {
        let s = SuiteId::new("core/nextest");
        assert_eq!(TestId::derive(&s, "a::b"), TestId::derive(&s, "a::b"));
        assert_ne!(TestId::derive(&s, "a::b"), TestId::derive(&s, "a::c"));
    }
}
