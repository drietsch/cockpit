//! Extraction and resolution of feature-id references from free text (test names, tags,
//! commit messages, ADR bodies).

use crate::ids::FeatureId;
use crate::model::Feature;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

static BRACKET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([A-Za-z][A-Za-z0-9._-]{1,63})\]").unwrap());
static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@?feat:([A-Za-z0-9][A-Za-z0-9._-]*)").unwrap());
static SUFFIX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"__feat__([a-z0-9_]+)").unwrap());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Bracket,
    Tag,
    FeatSuffix,
    Bare,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundRef {
    pub raw: String,
    pub resolved: Option<FeatureId>,
    pub kind: RefKind,
    pub ambiguous: Vec<FeatureId>,
}

pub struct FeatureIdIndex {
    ids: HashSet<FeatureId>,
    /// dots/dashes → underscores, for `__feat__` suffixes.
    flat: HashMap<String, Vec<FeatureId>>,
    aliases: HashMap<String, FeatureId>,
    pattern: Option<Regex>,
    bare: Option<aho_corasick::AhoCorasick>,
    bare_ids: Vec<FeatureId>,
}

impl FeatureIdIndex {
    pub fn build(features: &[Feature], id_pattern: Option<&str>) -> FeatureIdIndex {
        let mut ids = HashSet::new();
        let mut flat: HashMap<String, Vec<FeatureId>> = HashMap::new();
        let mut aliases = HashMap::new();
        for f in features {
            ids.insert(f.id.clone());
            flat.entry(flatten(f.id.as_str())).or_default().push(f.id.clone());
            for a in &f.aliases {
                aliases.insert(a.clone(), f.id.clone());
                flat.entry(flatten(a)).or_default().push(f.id.clone());
            }
        }
        let pattern = id_pattern.and_then(|p| Regex::new(p).ok());
        // Bare matching only for ids that are distinctive: at least 4 chars and containing
        // a separator (`FEAT-019`, `layers.ops`) — never `R0`.
        let bare_ids: Vec<FeatureId> = features
            .iter()
            .map(|f| f.id.clone())
            .filter(|id| id.as_str().len() >= 4 && id.as_str().contains(['-', '.', '_']))
            .collect();
        let bare = if bare_ids.is_empty() {
            None
        } else {
            aho_corasick::AhoCorasick::builder()
                .match_kind(aho_corasick::MatchKind::LeftmostLongest)
                .build(bare_ids.iter().map(|i| i.as_str()))
                .ok()
        };
        FeatureIdIndex { ids, flat, aliases, pattern, bare, bare_ids }
    }

    pub fn contains(&self, id: &FeatureId) -> bool {
        self.ids.contains(id)
    }

    pub fn ids(&self) -> &HashSet<FeatureId> {
        &self.ids
    }

    pub fn resolve(&self, raw: &str) -> Option<FeatureId> {
        let id = FeatureId::new(raw);
        if self.ids.contains(&id) {
            return Some(id);
        }
        if let Some(a) = self.aliases.get(raw) {
            return Some(a.clone());
        }
        None
    }

    pub fn resolve_flat(&self, flat_id: &str) -> Result<FeatureId, Vec<FeatureId>> {
        match self.flat.get(flat_id) {
            Some(v) if v.len() == 1 => Ok(v[0].clone()),
            Some(v) => Err(v.clone()),
            None => Err(vec![]),
        }
    }

    pub fn matches_pattern(&self, s: &str) -> bool {
        self.pattern.as_ref().is_some_and(|p| p.is_match(s))
    }

    /// Find references in text. Bracketed, tagged and suffixed forms are always considered;
    /// bare ids only when `allow_bare` (distinctive ids or a configured pattern).
    pub fn find_refs(&self, text: &str, allow_bare: bool) -> Vec<FoundRef> {
        let mut out: Vec<FoundRef> = Vec::new();
        let mut push = |r: FoundRef| {
            if !out.iter().any(|x| x.raw == r.raw && x.kind == r.kind) {
                out.push(r);
            }
        };
        for c in BRACKET_RE.captures_iter(text) {
            let raw = c[1].to_string();
            if raw.to_ascii_uppercase().starts_with("ADR") {
                continue;
            }
            let resolved = self.resolve(&raw);
            if resolved.is_some() || self.matches_pattern(&raw) || raw.contains(['-', '.']) {
                push(FoundRef { raw, resolved, kind: RefKind::Bracket, ambiguous: vec![] });
            }
        }
        for c in TAG_RE.captures_iter(text) {
            let raw = c[1].to_string();
            let resolved = self.resolve(&raw);
            push(FoundRef { raw, resolved, kind: RefKind::Tag, ambiguous: vec![] });
        }
        for c in SUFFIX_RE.captures_iter(text) {
            let raw = c[1].to_string();
            match self.resolve_flat(&raw) {
                Ok(id) => push(FoundRef {
                    raw,
                    resolved: Some(id),
                    kind: RefKind::FeatSuffix,
                    ambiguous: vec![],
                }),
                Err(amb) => push(FoundRef {
                    raw,
                    resolved: None,
                    kind: RefKind::FeatSuffix,
                    ambiguous: amb,
                }),
            }
        }
        if allow_bare {
            if let Some(ac) = &self.bare {
                for m in ac.find_iter(text) {
                    // Require word boundaries so `layers.ops` does not match `layers.opsx`.
                    let s = m.start();
                    let e = m.end();
                    let before_ok = s == 0 || !is_id_char(text.as_bytes()[s - 1]);
                    let after_ok = e >= text.len() || !is_id_char(text.as_bytes()[e]);
                    if before_ok && after_ok {
                        let id = self.bare_ids[m.pattern().as_usize()].clone();
                        push(FoundRef {
                            raw: id.0.clone(),
                            resolved: Some(id),
                            kind: RefKind::Bare,
                            ambiguous: vec![],
                        });
                    }
                }
            }
        }
        out
    }
}

fn is_id_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_'
}

pub fn flatten(id: &str) -> String {
    id.to_ascii_lowercase().replace(['.', '-'], "_")
}

/// Raw reference tokens (unresolved) as an importer sees them: `feat:x` tags, `__feat__x`
/// suffixes (kept as `flat:x`), and bracketed ids with a separator (`[FEAT-019]`).
pub fn raw_refs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for c in TAG_RE.captures_iter(text) {
        out.push(c[1].to_string());
    }
    for c in SUFFIX_RE.captures_iter(text) {
        out.push(format!("flat:{}", &c[1]));
    }
    for c in BRACKET_RE.captures_iter(text) {
        let raw = c[1].to_string();
        if !raw.to_ascii_uppercase().starts_with("ADR") && raw.contains(['-', '.']) {
            out.push(raw);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Importance, Lifecycle};
    use crate::paths::RelPath;

    fn f(id: &str) -> Feature {
        Feature {
            id: FeatureId::new(id),
            title: id.into(),
            description: None,
            lifecycle: Lifecycle::Stable,
            importance: Importance::Normal,
            owners: vec![],
            tags: vec![],
            paths: vec![],
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
            source: RelPath::new("x.md"),
            extra: Default::default(),
        }
    }

    #[test]
    fn finds_and_resolves() {
        let idx = FeatureIdIndex::build(
            &[f("FEAT-019"), f("layers.ops"), f("layers-ops-legacy"), f("R0")],
            None,
        );
        let r = idx.find_refs("feat(search): replace index [FEAT-019] and layers.ops, see [ADR-0022] @feat:layers.ops R0", true);
        let kinds: Vec<(String, bool)> =
            r.iter().map(|x| (x.raw.clone(), x.resolved.is_some())).collect();
        assert!(kinds.contains(&("FEAT-019".into(), true)));
        assert!(kinds.iter().any(|(raw, ok)| raw == "layers.ops" && *ok));
        assert!(!kinds.iter().any(|(raw, _)| raw == "ADR-0022"));
        assert!(!kinds.iter().any(|(raw, _)| raw == "R0"), "short ids never match bare");
        let s = idx.find_refs("tests::layer_insert__feat__layers_ops", false);
        assert_eq!(s[0].resolved, Some(FeatureId::new("layers.ops")));
        // ambiguous flat id
        let idx2 = FeatureIdIndex::build(&[f("a.b"), f("a-b")], None);
        let s = idx2.find_refs("x__feat__a_b", false);
        assert!(s[0].resolved.is_none() && s[0].ambiguous.len() == 2);
    }
}
