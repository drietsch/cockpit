use crate::ids::{AdrId, ComponentId, FeatureId};
use crate::paths::RelPath;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lifecycle {
    Planned,
    Experimental,
    Beta,
    Stable,
    Deprecated,
    Retired,
    Unspecified,
}

impl Lifecycle {
    pub fn parse(s: &str) -> Option<Lifecycle> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "planned" | "proposed" | "idea" => Lifecycle::Planned,
            "experimental" | "alpha" | "in-progress" | "in_progress" | "wip" | "partial" => {
                Lifecycle::Experimental
            }
            "beta" | "preview" => Lifecycle::Beta,
            "stable" | "shipped" | "ga" | "done" | "released" => Lifecycle::Stable,
            "deprecated" => Lifecycle::Deprecated,
            "retired" | "removed" | "archived" => Lifecycle::Retired,
            "unspecified" | "" => Lifecycle::Unspecified,
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Lifecycle::Planned => "planned",
            Lifecycle::Experimental => "experimental",
            Lifecycle::Beta => "beta",
            Lifecycle::Stable => "stable",
            Lifecycle::Deprecated => "deprecated",
            Lifecycle::Retired => "retired",
            Lifecycle::Unspecified => "unspecified",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Importance {
    Critical,
    High,
    Normal,
    Low,
}

impl Importance {
    pub fn parse(s: &str) -> Option<Importance> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "critical" | "p0" => Importance::Critical,
            "high" | "p1" => Importance::High,
            "normal" | "medium" | "p2" | "" => Importance::Normal,
            "low" | "p3" => Importance::Low,
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Importance::Critical => "critical",
            Importance::High => "high",
            Importance::Normal => "normal",
            Importance::Low => "low",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaimStatus {
    Shipped,
    Partial,
    Planned,
}

impl ClaimStatus {
    pub fn parse(s: &str) -> Option<ClaimStatus> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "shipped" | "done" | "stable" | "yes" => ClaimStatus::Shipped,
            "partial" | "wip" | "in-progress" => ClaimStatus::Partial,
            "planned" | "todo" | "no" => ClaimStatus::Planned,
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaimStatus::Shipped => "shipped",
            ClaimStatus::Partial => "partial",
            ClaimStatus::Planned => "planned",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub component: ComponentId,
    pub status: ClaimStatus,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    pub index: u32,
    pub text: String,
    pub checked: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub level: u8,
    pub heading: String,
    pub body_md: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Feature {
    pub id: FeatureId,
    pub title: String,
    pub description: Option<String>,
    pub lifecycle: Lifecycle,
    pub importance: Importance,
    pub owners: Vec<String>,
    pub tags: Vec<String>,
    /// Globs, project-root-relative.
    pub paths: Vec<String>,
    pub adr_ids: Vec<AdrId>,
    pub dependencies: Vec<FeatureId>,
    pub aliases: Vec<String>,
    pub claims: Vec<Claim>,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub sections: Vec<Section>,
    pub body_md: String,
    pub since: Option<String>,
    pub superseded_by: Option<FeatureId>,
    pub references: Vec<String>,
    /// Subdirectory chain under the features dir (`["layers"]`).
    pub group: Vec<String>,
    pub source: RelPath,
    /// Unknown frontmatter keys, preserved.
    pub extra: BTreeMap<String, serde_json::Value>,
}
