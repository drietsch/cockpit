//! Feature specifications: Markdown with YAML frontmatter (§17).

use super::frontmatter;
use super::markdown::{self as md, Heading};
use super::{
    ImportContext, ImportInput, ImportOutput, ImportedEntity, Importer, ImporterKind,
    yaml_scalar_string, yaml_string_list, yaml_to_json,
};
use crate::ids::{AdrId, ComponentId, FeatureId};
use crate::model::*;
use crate::paths::RelPath;
use serde_yaml_ng::Value as Y;
use std::collections::BTreeMap;

pub struct FeatureMdImporter;

const NAME: &str = "feature-md";

const KNOWN_KEYS: &[&str] = &[
    "id",
    "title",
    "status",
    "lifecycle",
    "importance",
    "priority",
    "owners",
    "owner",
    "tags",
    "tag",
    "paths",
    "path",
    "adrs",
    "adr",
    "dependencies",
    "depends_on",
    "aliases",
    "claims",
    "components",
    "since",
    "superseded_by",
    "spec",
    "references",
    "notes",
    "description",
    "metadata",
];

impl Importer for FeatureMdImporter {
    fn kind(&self) -> ImporterKind {
        ImporterKind::FeatureMd
    }
    fn version(&self) -> u32 {
        1
    }
    fn import(&self, ctx: &ImportContext, input: &ImportInput) -> ImportOutput {
        let mut out = ImportOutput::default();
        let text = input.text();
        let split = frontmatter::split(&text);
        let mut fm = serde_yaml_ng::Mapping::new();
        match split.yaml {
            Some(y) => match frontmatter::parse_yaml(y) {
                Ok(m) => fm = m,
                Err(e) => {
                    out.fatal(
                        &input.rel,
                        NAME,
                        format!("frontmatter does not parse: {e}"),
                        Some("line 1".into()),
                    );
                    return out;
                }
            },
            None => {
                out.warn(&input.rel, NAME, "no YAML frontmatter; using the filename as id", None)
            }
        }
        let get = |k: &str| fm.get(Y::String(k.to_string()));
        let get_str = |k: &str| {
            get(k)
                .and_then(yaml_scalar_string)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };

        let lines = md::lines(split.body);
        let heads = md::headings(&lines);
        let h1 = heads.iter().find(|h| h.level == 1);

        let id = get_str("id").unwrap_or_else(|| input.rel.file_stem().to_string());
        let title = get_str("title")
            .or_else(|| h1.map(|h| h.text.clone()))
            .unwrap_or_else(|| input.rel.file_stem().to_string());

        let features_cfg = &ctx.located.config.features;
        let lifecycle_raw = get_str("status").or_else(|| get_str("lifecycle"));
        let lifecycle = match &lifecycle_raw {
            Some(s) => match Lifecycle::parse(s) {
                Some(l) => l,
                None => {
                    out.warn(
                        &input.rel,
                        NAME,
                        format!("unknown status/lifecycle {s:?}; treating as unspecified"),
                        None,
                    );
                    Lifecycle::Unspecified
                }
            },
            None => {
                Lifecycle::parse(&features_cfg.default_lifecycle).unwrap_or(Lifecycle::Unspecified)
            }
        };
        let importance_raw = get_str("importance").or_else(|| get_str("priority"));
        let importance = match &importance_raw {
            Some(s) => match Importance::parse(s) {
                Some(i) => i,
                None => {
                    out.warn(
                        &input.rel,
                        NAME,
                        format!("unknown importance {s:?}; treating as normal"),
                        None,
                    );
                    Importance::Normal
                }
            },
            None => {
                Importance::parse(&features_cfg.default_importance).unwrap_or(Importance::Normal)
            }
        };

        let owners = merge(yaml_string_list(get("owners")), yaml_string_list(get("owner")));
        let mut tags = merge(yaml_string_list(get("tags")), yaml_string_list(get("tag")));
        let paths = merge(yaml_string_list(get("paths")), yaml_string_list(get("path")));
        for p in &paths {
            if globset::Glob::new(p).is_err() {
                out.warn(&input.rel, NAME, format!("invalid path glob {p:?}"), None);
            }
        }
        let mut adr_ids: Vec<AdrId> = Vec::new();
        for raw in merge(yaml_string_list(get("adrs")), yaml_string_list(get("adr"))) {
            match AdrId::parse_ref(&raw) {
                Some(a) => adr_ids.push(a),
                None => out.warn(
                    &input.rel,
                    NAME,
                    format!("adrs entry {raw:?} is not an ADR reference"),
                    None,
                ),
            }
        }
        let dependencies: Vec<FeatureId> =
            merge(yaml_string_list(get("dependencies")), yaml_string_list(get("depends_on")))
                .into_iter()
                .map(FeatureId::new)
                .collect();
        let aliases = yaml_string_list(get("aliases"));

        let mut claims: Vec<Claim> = Vec::new();
        if let Some(Y::Mapping(m)) = get("claims") {
            for (k, v) in m {
                let Some(comp) = yaml_scalar_string(k) else { continue };
                let (status_raw, note) = match v {
                    Y::Mapping(inner) => (
                        inner.get(Y::String("status".into())).and_then(yaml_scalar_string),
                        inner.get(Y::String("note".into())).and_then(yaml_scalar_string),
                    ),
                    other => (yaml_scalar_string(other), None),
                };
                match status_raw.as_deref().and_then(ClaimStatus::parse) {
                    Some(status) => {
                        claims.push(Claim { component: ComponentId::new(comp), status, note })
                    }
                    None => out.warn(
                        &input.rel,
                        NAME,
                        format!("claim {comp:?} has no valid status (shipped|partial|planned)"),
                        None,
                    ),
                }
            }
        }
        for comp in yaml_string_list(get("components")) {
            if !claims.iter().any(|c| c.component.as_str() == comp) {
                claims.push(Claim {
                    component: ComponentId::new(comp),
                    status: ClaimStatus::Shipped,
                    note: None,
                });
            }
        }
        if !ctx.project.components.is_empty() {
            for c in &claims {
                if !ctx.project.components.iter().any(|k| k.id == c.component) {
                    out.warn(
                        &input.rel,
                        NAME,
                        format!("claim refers to unknown component {:?}", c.component.as_str()),
                        None,
                    );
                }
            }
        }

        let description =
            get_str("description").or_else(|| md::first_paragraph(&lines, h1.map(|h| h.line)));
        let acceptance_criteria = acceptance_criteria(&lines, &heads);
        let sections = md::sections(&lines, &heads);

        // Group = subdirectory chain below the features root.
        let features_root = RelPath::new(&features_cfg.path);
        let group: Vec<String> = input
            .rel
            .parent()
            .strip_prefix(&features_root)
            .map(|p| p.components().map(|s| s.to_string()).collect())
            .unwrap_or_default();
        for g in &group {
            if !tags.contains(g) {
                tags.push(g.clone());
            }
        }

        let mut extra: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        for (k, v) in &fm {
            if let Some(key) = yaml_scalar_string(k) {
                if !KNOWN_KEYS.contains(&key.as_str()) {
                    extra.insert(key, yaml_to_json(v));
                }
            }
        }
        if let Some(m) = get("metadata") {
            extra.insert("metadata".into(), yaml_to_json(m));
        }

        let references = merge(yaml_string_list(get("references")), yaml_string_list(get("spec")));
        let body_md = split.body.trim().to_string();
        let feature = Feature {
            id: FeatureId::new(id),
            title,
            description,
            lifecycle,
            importance,
            owners,
            tags,
            paths,
            adr_ids,
            dependencies,
            aliases,
            claims,
            acceptance_criteria,
            sections,
            body_md,
            since: get_str("since"),
            superseded_by: get_str("superseded_by").map(FeatureId::new),
            references,
            group,
            source: input.rel.clone(),
            extra,
        };
        out.entities.push(ImportedEntity::Feature(feature));
        out
    }
}

fn merge(mut a: Vec<String>, b: Vec<String>) -> Vec<String> {
    for x in b {
        if !a.contains(&x) {
            a.push(x);
        }
    }
    a
}

fn acceptance_criteria(lines: &[&str], heads: &[Heading]) -> Vec<AcceptanceCriterion> {
    let Some(h) = heads.iter().find(|h| {
        let n = md::norm_heading(&h.text);
        matches!(
            n.as_str(),
            "acceptance criteria" | "acceptance" | "ac" | "criteria" | "acceptance tests"
        )
    }) else {
        return vec![];
    };
    md::list_items_under(lines, h)
        .into_iter()
        .enumerate()
        .map(|(i, (text, checked))| AcceptanceCriterion { index: i as u32 + 1, text, checked })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::config::{Config, Located};
    use crate::hash::ContentHash;
    use std::collections::HashSet;

    pub(crate) fn located() -> Located {
        Located {
            root: std::path::PathBuf::from("/p"),
            config_path: None,
            config: Config::default(),
            state_dir: std::path::PathBuf::from("/p/.cockpit"),
            implicit: true,
        }
    }

    pub(crate) fn project() -> Project {
        Project {
            id: "p".into(),
            name: "P".into(),
            root: "/p".into(),
            repos: vec![],
            components: vec![],
            primary_branch: "main".into(),
        }
    }

    pub(crate) fn input(rel: &str, text: &str) -> ImportInput {
        ImportInput {
            abs: std::path::PathBuf::from("/p").join(rel),
            rel: RelPath::new(rel),
            bytes: text.as_bytes().to_vec(),
            content_hash: ContentHash::of(text.as_bytes()),
            mtime: crate::time::parse_timestamp("2026-09-11T10:00:00Z").unwrap(),
            size: text.len() as u64,
        }
    }

    pub(crate) fn run(rel: &str, text: &str) -> ImportOutput {
        let loc = located();
        let proj = project();
        let repos = super::super::RepoIndex::default();
        let ids: HashSet<FeatureId> = HashSet::new();
        let ctx = ImportContext {
            located: &loc,
            project: &proj,
            repos: &repos,
            feature_ids: &ids,
            source: None,
            source_id: None,
            now: crate::time::parse_timestamp("2026-09-11T12:00:00Z").unwrap(),
            cargo: &Default::default(),
            sidecars: &Default::default(),
            commits: &Default::default(),
        };
        FeatureMdImporter.import(&ctx, &input(rel, text))
    }

    const BRIEFING_EXAMPLE: &str = "---\nid: FEAT-019\ntitle: Search\nstatus: stable\nimportance: high\n\nowners:\n  - search\n\ntags:\n  - user-facing\n  - core\n\npaths:\n  - src/search/**\n  - src/index/**\n\nadrs:\n  - ADR-016\n  - ADR-022\nclaims:\n  core.parser: shipped\n  editor.panel: { status: partial, note: \"grid only\" }\nx-custom: 42\n---\n\n# Search\n\nAllows users to search invoices, clients, and projects.\n\n## Acceptance criteria\n\n- Search results appear within 200 ms for standard workloads.\n- Matching should tolerate minor spelling errors.\n- Search results must respect user permissions.\n";

    #[test]
    fn imports_the_briefing_example() {
        let out = run("docs/features/FEAT-019-search.md", BRIEFING_EXAMPLE);
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        let ImportedEntity::Feature(f) = &out.entities[0] else { panic!() };
        assert_eq!(f.id.as_str(), "FEAT-019");
        assert_eq!(f.title, "Search");
        assert_eq!(f.lifecycle, Lifecycle::Stable);
        assert_eq!(f.importance, Importance::High);
        assert_eq!(f.owners, vec!["search"]);
        assert_eq!(f.paths, vec!["src/search/**", "src/index/**"]);
        assert_eq!(
            f.adr_ids.iter().map(|a| a.as_str()).collect::<Vec<_>>(),
            vec!["ADR-016", "ADR-022"]
        );
        assert_eq!(
            f.description.as_deref(),
            Some("Allows users to search invoices, clients, and projects.")
        );
        assert_eq!(f.acceptance_criteria.len(), 3);
        assert_eq!(f.claims.len(), 2);
        assert_eq!(f.claims[1].status, ClaimStatus::Partial);
        assert_eq!(f.claims[1].note.as_deref(), Some("grid only"));
        assert_eq!(f.extra["x-custom"], serde_json::json!(42));
        assert!(f.group.is_empty());
    }

    #[test]
    fn grouped_dotted_ids_and_fallbacks() {
        let out = run(
            "docs/features/layers/layers.ops.md",
            "---\nid: layers.ops\ntitle: Layer ops\nstatus: shipped\n---\n# Layer ops\n",
        );
        let ImportedEntity::Feature(f) = &out.entities[0] else { panic!() };
        assert_eq!(f.group, vec!["layers"]);
        assert!(f.tags.contains(&"layers".to_string()));
        assert_eq!(f.lifecycle, Lifecycle::Stable);

        let out = run("docs/features/no-fm.md", "# Just a title\n\nSome text.\n");
        assert_eq!(out.warnings.len(), 1);
        let ImportedEntity::Feature(f) = &out.entities[0] else { panic!() };
        assert_eq!(f.id.as_str(), "no-fm");
        assert_eq!(f.title, "Just a title");

        let out = run("docs/features/bad.md", "---\n: : :\n---\n# x\n");
        assert!(out.entities.is_empty());
        assert!(out.warnings[0].fatal);
    }
}
