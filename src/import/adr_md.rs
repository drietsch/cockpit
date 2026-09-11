//! Architecture Decision Records. Accepts YAML frontmatter, Guardian's bold-label header
//! (`**Status:** Accepted`, `**Date:** 2026-…`) and paged's inline form
//! (`**2026-06-07 · decision record · status: ACCEPTED**`, `- **Status:** PROPOSED (date)`).

use super::frontmatter;
use super::markdown::{self as md};
use super::{
    ImportContext, ImportInput, ImportOutput, ImportedEntity, Importer, ImporterKind,
    yaml_scalar_string, yaml_string_list,
};
use crate::ids::{AdrId, FeatureId};
use crate::model::*;
use crate::time::parse_date;
use regex::Regex;
use serde_yaml_ng::Value as Y;
use std::sync::LazyLock;

pub struct AdrMdImporter;

const NAME: &str = "adr-md";

static H1_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^ADR[-\s_]?(\d+)\s*(?:[—–:\-]\s*)?(.*)$").unwrap());
static FILE_NUM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d{1,6})[-_. ]").unwrap());
static STATUS_BOLD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\*\*Status:?\*\*:?\s*(.+)").unwrap());
static STATUS_INLINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\*\*(\d{4}-\d{2}-\d{2})\s*·.*?status:\s*([A-Za-z]+)").unwrap()
});
static STATUS_PLAIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*-?\s*status:\s*(.+)$").unwrap());
static DATE_BOLD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\*\*Date:?\*\*:?\s*(\d{4}-\d{2}-\d{2})").unwrap());
static DATE_ANY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{4}-\d{2}-\d{2})").unwrap());
static ADR_REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bADR[-\s_]?(\d{1,6})\b").unwrap());
static SUPERSEDES_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\*\*Supersedes:?\*\*:?\s*(.+)").unwrap());
static EVIDENCE_H2_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^Evidence(?:\s*[—–:\-]\s*(.+?))?(?:\s*\((\d{4}-\d{2}-\d{2})\))?$").unwrap()
});
static BRACKET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([A-Za-z][A-Za-z0-9._-]{1,63})\]").unwrap());

impl Importer for AdrMdImporter {
    fn kind(&self) -> ImporterKind {
        ImporterKind::AdrMd
    }
    fn version(&self) -> u32 {
        1
    }
    fn import(&self, ctx: &ImportContext, input: &ImportInput) -> ImportOutput {
        let mut out = ImportOutput::default();
        let text = input.text();
        let split = frontmatter::split(&text);
        let mut fm = serde_yaml_ng::Mapping::new();
        if let Some(y) = split.yaml {
            match frontmatter::parse_yaml(y) {
                Ok(m) => fm = m,
                Err(e) => out.warn(
                    &input.rel,
                    NAME,
                    format!("frontmatter does not parse: {e}"),
                    Some("line 1".into()),
                ),
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

        // Identity: frontmatter id → H1 "ADR-0057 — Title" → filename number.
        let mut number_str: Option<String> = None;
        let mut title: Option<String> = get_str("title");
        if let Some(h) = h1 {
            if let Some(c) = H1_RE.captures(&h.text) {
                number_str = Some(c[1].to_string());
                let t = c[2].trim();
                if title.is_none() && !t.is_empty() {
                    title = Some(t.to_string());
                }
            } else if title.is_none() {
                title = Some(h.text.clone());
            }
        }
        let id = match get_str("id").and_then(|s| AdrId::parse_ref(&s)) {
            Some(i) => i,
            None => match number_str.as_deref().or_else(|| {
                FILE_NUM_RE.captures(input.rel.file_name()).map(|c| c.get(1).unwrap().as_str())
            }) {
                Some(n) => AdrId::new(format!("ADR-{n}")),
                None => {
                    out.warn(
                        &input.rel,
                        NAME,
                        "no ADR number in frontmatter, H1 or filename; using the filename",
                        None,
                    );
                    AdrId::new(input.rel.file_stem().to_string())
                }
            },
        };
        let title = title.unwrap_or_else(|| input.rel.file_stem().to_string());

        // Status: frontmatter, else scan the first 40 body lines after the H1.
        let scan_from = h1.map(|h| h.line + 1).unwrap_or(0);
        let window: Vec<&str> = lines.iter().skip(scan_from).take(40).copied().collect();
        let mut status_raw: Option<String> = get_str("status");
        let mut date: Option<jiff::civil::Date> = get_str("date").and_then(|d| parse_date(&d));
        if status_raw.is_none() {
            for (i, line) in window.iter().enumerate() {
                if let Some(c) = STATUS_INLINE_RE.captures(line) {
                    status_raw = Some(c[2].to_string());
                    if date.is_none() {
                        date = parse_date(&c[1]);
                    }
                    break;
                }
                if let Some(c) = STATUS_BOLD_RE.captures(line) {
                    // A bold status line may wrap: take the rest of the paragraph.
                    let mut s = c[1].trim().to_string();
                    for cont in window.iter().skip(i + 1) {
                        if cont.trim().is_empty()
                            || cont.trim().starts_with("**")
                            || cont.trim().starts_with('#')
                        {
                            break;
                        }
                        s.push(' ');
                        s.push_str(cont.trim());
                    }
                    status_raw = Some(s);
                    break;
                }
                if let Some(c) = STATUS_PLAIN_RE.captures(line) {
                    status_raw = Some(c[1].trim().to_string());
                    break;
                }
            }
        }
        if date.is_none() {
            for line in &window {
                if let Some(c) = DATE_BOLD_RE.captures(line) {
                    date = parse_date(&c[1]);
                    break;
                }
            }
        }
        let status_raw = status_raw.unwrap_or_default();
        let (status, superseded_by, accepted_at) = normalise_status(&status_raw);
        if status == AdrStatus::Unknown {
            out.warn(
                &input.rel,
                NAME,
                if status_raw.is_empty() {
                    "no status found".to_string()
                } else {
                    format!("unrecognised status {status_raw:?}")
                },
                None,
            );
        }
        let accepted_at = accepted_at.or(if status == AdrStatus::Accepted { date } else { None });
        let superseded_by =
            get_str("superseded_by").and_then(|s| AdrId::parse_ref(&s)).or(superseded_by);

        let mut supersedes: Vec<AdrId> = yaml_string_list(get("supersedes"))
            .iter()
            .filter_map(|s| AdrId::parse_ref(s))
            .collect();
        for line in &window {
            if let Some(c) = SUPERSEDES_RE.captures(line) {
                for r in ADR_REF_RE.captures_iter(&c[1]) {
                    let a = AdrId::new(format!("ADR-{}", &r[1]));
                    if !supersedes.contains(&a) {
                        supersedes.push(a);
                    }
                }
            }
        }

        let mut adr_refs: Vec<AdrId> = Vec::new();
        for c in ADR_REF_RE.captures_iter(split.body) {
            let a = AdrId::new(format!("ADR-{}", &c[1]));
            if a != id && !adr_refs.contains(&a) {
                adr_refs.push(a);
            }
        }

        let feature_ids: Vec<FeatureId> =
            yaml_string_list(get("features")).into_iter().map(FeatureId::new).collect();
        let mut mentioned_tokens: Vec<String> = Vec::new();
        for c in BRACKET_RE.captures_iter(split.body) {
            let t = c[1].to_string();
            if !t.to_ascii_uppercase().starts_with("ADR") && !mentioned_tokens.contains(&t) {
                mentioned_tokens.push(t);
            }
        }
        for fid in ctx.feature_ids {
            if fid.as_str().len() >= 4
                && split.body.contains(fid.as_str())
                && !mentioned_tokens.contains(&fid.0)
            {
                mentioned_tokens.push(fid.0.clone());
            }
        }

        let sections = md::sections(&lines, &heads);
        let evidence_notes: Vec<EvidenceNote> = heads
            .iter()
            .filter(|h| h.level == 2)
            .filter_map(|h| {
                let c = EVIDENCE_H2_RE.captures(&h.text)?;
                let body = sections
                    .iter()
                    .find(|s| s.heading == h.text)
                    .map(|s| s.body_md.clone())
                    .unwrap_or_default();
                let date = c
                    .get(2)
                    .and_then(|m| parse_date(m.as_str()))
                    .or_else(|| DATE_ANY_RE.captures(&h.text).and_then(|d| parse_date(&d[1])));
                Some(EvidenceNote { heading: h.text.clone(), date, body_md: body })
            })
            .collect();

        let number = id.number();
        out.entities.push(ImportedEntity::Adr(Adr {
            id,
            number,
            title,
            status,
            status_raw,
            date,
            accepted_at,
            superseded_by,
            supersedes,
            adr_refs,
            feature_ids,
            mentioned_tokens,
            evidence_notes,
            sections,
            body_md: split.body.trim().to_string(),
            tags: yaml_string_list(get("tags")),
            source: input.rel.clone(),
        }));
        out
    }
}

/// Map free-form status text to the enum; also extracts `superseded by ADR-x` and an
/// acceptance date when one is written next to the status.
pub fn normalise_status(raw: &str) -> (AdrStatus, Option<AdrId>, Option<jiff::civil::Date>) {
    let cleaned: String = raw
        .trim()
        .trim_start_matches(|c: char| {
            c == '*' || c == '_' || c == '`' || c == '[' || c == '(' || c == '"' || c == '\''
        })
        .replace(['*', '_', '`'], "");
    if cleaned.contains('|') && cleaned.to_ascii_lowercase().contains("proposed") {
        return (AdrStatus::Unknown, None, None); // template placeholder
    }
    let first = cleaned
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let status = match first.as_str() {
        "accepted" | "ratified" | "approved" | "adopted" | "implemented" | "done" | "final"
        | "active" => AdrStatus::Accepted,
        "proposed" | "open" | "pending" | "review" | "rfc" => AdrStatus::Proposed,
        "draft" | "wip" => AdrStatus::Draft,
        "superseded" | "replaced" => AdrStatus::Superseded,
        "rejected" | "declined" => AdrStatus::Rejected,
        "deprecated" | "retired" | "obsolete" | "withdrawn" | "abandoned" => AdrStatus::Deprecated,
        _ => AdrStatus::Unknown,
    };
    let superseded_by = if status == AdrStatus::Superseded {
        ADR_REF_RE.captures(&cleaned).map(|c| AdrId::new(format!("ADR-{}", &c[1])))
    } else {
        None
    };
    let date = if status == AdrStatus::Accepted {
        DATE_ANY_RE.captures(&cleaned).and_then(|c| parse_date(&c[1]))
    } else {
        None
    };
    (status, superseded_by, date)
}

#[cfg(test)]
mod tests {
    use super::super::feature_md::tests::{input, located, project};
    use super::*;
    use std::collections::HashSet;

    fn run(rel: &str, text: &str, known: &[&str]) -> Adr {
        let loc = located();
        let proj = project();
        let repos = super::super::RepoIndex::default();
        let ids: HashSet<FeatureId> = known.iter().map(|s| FeatureId::new(*s)).collect();
        let ctx = ImportContext {
            located: &loc,
            project: &proj,
            repos: &repos,
            feature_ids: &ids,
            source: None,
            source_id: None,
            now: crate::time::parse_timestamp("2026-09-11T12:00:00Z").unwrap(),
        };
        let out = AdrMdImporter.import(&ctx, &input(rel, text));
        match out.entities.into_iter().next() {
            Some(ImportedEntity::Adr(a)) => a,
            _ => panic!("no adr"),
        }
    }

    #[test]
    fn guardian_style_wrapped_bold_status() {
        let a = run(
            "docs/adr/0057-a-performance-budget.md",
            "# ADR-0057 — A performance budget is a contract, or it is a claim\n\n**Status:** **Accepted (2026-08-22)** — the plane,\nas built, discharges the gate\n**Date:** 2026-09-08\n**Freeze gate:** one pull request blocked\n**Sources:** docs/36\n\n## Context\n\nSee ADR-0015 and [FEAT-024].\n\n## Decision\n\nx\n\n## Evidence — PC-1 (2026-09-08)\n\nmeasured.\n\n## Evidence\n\nmore\n",
            &["FEAT-024"],
        );
        assert_eq!(a.id.as_str(), "ADR-0057");
        assert_eq!(a.number, Some(57));
        assert_eq!(a.title, "A performance budget is a contract, or it is a claim");
        assert_eq!(a.status, AdrStatus::Accepted);
        assert_eq!(a.accepted_at.map(|d| d.to_string()), Some("2026-08-22".into()));
        assert_eq!(a.date.map(|d| d.to_string()), Some("2026-09-08".into()));
        assert_eq!(a.adr_refs, vec![AdrId::new("ADR-0015")]);
        assert_eq!(a.mentioned_tokens, vec!["FEAT-024"]);
        assert_eq!(a.evidence_notes.len(), 2);
        assert_eq!(a.evidence_notes[0].date.map(|d| d.to_string()), Some("2026-09-08".into()));
        assert_eq!(a.evidence_notes[0].body_md, "measured.");
    }

    #[test]
    fn superseded_template_and_paged_forms() {
        let a = run(
            "docs/adr/0016-x.md",
            "# ADR-0016 — Old\n\n**Status:** Superseded by [ADR-0022](0022-y.md) (2026-08-20)\n",
            &[],
        );
        assert_eq!(a.status, AdrStatus::Superseded);
        assert_eq!(a.superseded_by, Some(AdrId::new("ADR-0022")));

        let a = run(
            "docs/adr/0000-template.md",
            "# ADR-NNNN — Title\n\n**Status:** Proposed | Accepted | Superseded by ADR-XXXX\n",
            &[],
        );
        assert_eq!(a.status, AdrStatus::Unknown);
        assert_eq!(a.id.as_str(), "ADR-0000");

        let a = run(
            "thoughts/docs/paged/adr/009-stage-matrix.md",
            "# ADR 009 — The stage matrix\n\n**2026-06-07 · decision record · status: ACCEPTED (v2 mandate)**\n\n**Sources:** x\n",
            &[],
        );
        assert_eq!(a.id.as_str(), "ADR-009");
        assert_eq!(a.status, AdrStatus::Accepted);
        assert_eq!(a.date.map(|d| d.to_string()), Some("2026-06-07".into()));

        let a = run(
            "thoughts/docs/paged/adr/025-journal.md",
            "# ADR 025 — Journal\n\n- **Status:** PROPOSED (2026-08-22)\n- **Supersedes:** ADR 011\n",
            &[],
        );
        assert_eq!(a.status, AdrStatus::Proposed);
        assert_eq!(a.supersedes, vec![AdrId::new("ADR-011")]);

        let a = run(
            "docs/adr/0003-frontmatter.md",
            "---\nid: ADR-3\ntitle: FM style\nstatus: accepted\ndate: 2026-01-02\nfeatures: [FEAT-1]\n---\n# FM style\n",
            &[],
        );
        assert_eq!(a.id.as_str(), "ADR-3");
        assert_eq!(a.status, AdrStatus::Accepted);
        assert_eq!(a.feature_ids, vec![FeatureId::new("FEAT-1")]);
        assert_eq!(a.accepted_at.map(|d| d.to_string()), Some("2026-01-02".into()));
    }
}
