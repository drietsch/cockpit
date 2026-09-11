//! Path ownership: which features claim a path through their `paths:` globs.

use crate::ids::FeatureId;
use crate::model::Feature;
use crate::paths::RelPath;
use globset::{Glob, GlobSet, GlobSetBuilder};

pub struct PathOwnership {
    set: GlobSet,
    /// glob index → (feature, glob text)
    owners: Vec<(FeatureId, String)>,
}

impl PathOwnership {
    pub fn build(features: &[Feature]) -> PathOwnership {
        let mut b = GlobSetBuilder::new();
        let mut owners = Vec::new();
        for f in features {
            for p in &f.paths {
                let text = normalise_glob(p);
                if let Ok(g) = Glob::new(&text) {
                    b.add(g);
                    owners.push((f.id.clone(), p.clone()));
                }
            }
        }
        PathOwnership { set: b.build().unwrap_or_else(|_| GlobSet::empty()), owners }
    }

    /// Every (feature, matching glob) for a path.
    pub fn owners_of(&self, path: &RelPath) -> Vec<(&FeatureId, &str)> {
        let mut out: Vec<(&FeatureId, &str)> = Vec::new();
        for i in self.set.matches(path.as_str()) {
            let (f, g) = &self.owners[i];
            if !out.iter().any(|(id, _)| *id == f) {
                out.push((f, g.as_str()));
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.owners.is_empty()
    }
}

/// `src/search` (a directory) should match everything beneath it; `src/search/**` already
/// does; a trailing slash is normalised the same way.
pub fn normalise_glob(p: &str) -> String {
    let p = p.trim().trim_start_matches("./");
    if p.ends_with('/') {
        return format!("{p}**");
    }
    if !p.contains('*') && !p.contains('?') && !p.contains('[') && !p.contains('.') {
        return format!("{p}/**");
    }
    p.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_globs_and_dirs() {
        let mk = |id: &str, paths: &[&str]| Feature {
            id: FeatureId::new(id),
            title: id.into(),
            description: None,
            lifecycle: crate::model::Lifecycle::Stable,
            importance: crate::model::Importance::Normal,
            owners: vec![],
            tags: vec![],
            paths: paths.iter().map(|s| s.to_string()).collect(),
            adr_ids: vec![],
            dependencies: vec![],
            aliases: vec![],
            claims: vec![],
            acceptance_criteria: vec![],
            sections: vec![],
            body_md: String::new(),
            since: None,
            superseded_by: None,
            references: vec![],
            group: vec![],
            source: RelPath::new("docs/features/x.md"),
            extra: Default::default(),
        };
        let o = PathOwnership::build(&[
            mk("A", &["src/search/**", "src/index/**"]),
            mk("B", &["crates/platform/guardian-retention"]),
        ]);
        assert_eq!(o.owners_of(&RelPath::new("src/search/index.rs")).len(), 1);
        assert_eq!(
            o.owners_of(&RelPath::new("crates/platform/guardian-retention/src/lib.rs"))[0]
                .0
                .as_str(),
            "B"
        );
        assert!(o.owners_of(&RelPath::new("docs/x.md")).is_empty());
        assert_eq!(normalise_glob("src/"), "src/**");
    }
}
