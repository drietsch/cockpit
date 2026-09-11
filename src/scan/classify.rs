//! Map a project-relative path to the importer(s) responsible for it.

use crate::config::{EvidenceFormat, Located};
use crate::import::ImporterKind;
use crate::paths::RelPath;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImporterFor {
    pub kind: ImporterKind,
    pub source_id: Option<String>,
}

/// Documents only (features / ADRs); evidence classification is added with the evidence walk.
pub fn classify_document(located: &Located, rel: &RelPath) -> Option<ImporterFor> {
    if rel.extension() != Some("md") {
        return None;
    }
    let name = rel.file_name();
    if name.starts_with('_') || name == "README.md" || name == "index.md" {
        return None;
    }
    let features_root = RelPath::new(&located.config.features.path);
    if rel.starts_with(&features_root) {
        return Some(ImporterFor { kind: ImporterKind::FeatureMd, source_id: None });
    }
    for p in located.config.adrs.path.items() {
        if rel.starts_with(&RelPath::new(&p)) {
            if located.config.adrs.exclude.iter().any(|x| x == name) {
                return None;
            }
            return Some(ImporterFor { kind: ImporterKind::AdrMd, source_id: None });
        }
    }
    None
}

pub fn format_for_kind(kind: ImporterKind) -> Option<EvidenceFormat> {
    match kind {
        ImporterKind::Evidence(f) => Some(f),
        _ => None,
    }
}
