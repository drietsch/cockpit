use crate::ids::{AdrId, FeatureId};
use crate::model::feature::Section;
use crate::paths::RelPath;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdrStatus {
    Draft,
    Proposed,
    Accepted,
    Rejected,
    Deprecated,
    Superseded,
    Unknown,
}

impl AdrStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AdrStatus::Draft => "draft",
            AdrStatus::Proposed => "proposed",
            AdrStatus::Accepted => "accepted",
            AdrStatus::Rejected => "rejected",
            AdrStatus::Deprecated => "deprecated",
            AdrStatus::Superseded => "superseded",
            AdrStatus::Unknown => "unknown",
        }
    }
    pub fn parse(s: &str) -> AdrStatus {
        match s {
            "draft" => AdrStatus::Draft,
            "proposed" => AdrStatus::Proposed,
            "accepted" => AdrStatus::Accepted,
            "rejected" => AdrStatus::Rejected,
            "deprecated" => AdrStatus::Deprecated,
            "superseded" => AdrStatus::Superseded,
            _ => AdrStatus::Unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvidenceNote {
    pub heading: String,
    pub date: Option<jiff::civil::Date>,
    pub body_md: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Adr {
    pub id: AdrId,
    pub number: Option<u32>,
    pub title: String,
    pub status: AdrStatus,
    pub status_raw: String,
    pub date: Option<jiff::civil::Date>,
    pub accepted_at: Option<jiff::civil::Date>,
    pub superseded_by: Option<AdrId>,
    pub supersedes: Vec<AdrId>,
    pub adr_refs: Vec<AdrId>,
    /// Explicit (frontmatter) feature links.
    pub feature_ids: Vec<FeatureId>,
    /// Raw tokens found in the body that may be feature ids; resolved by the linker.
    pub mentioned_tokens: Vec<String>,
    pub evidence_notes: Vec<EvidenceNote>,
    pub sections: Vec<Section>,
    pub body_md: String,
    pub tags: Vec<String>,
    pub source: RelPath,
}
