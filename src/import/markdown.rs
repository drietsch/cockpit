//! Light, line-based Markdown structure helpers (headings, first paragraph, list items,
//! sections). Rendering to HTML lives in the server layer via comrak; here we only need the
//! outline, and a line scanner that respects fenced code blocks is more robust for the
//! real-world documents we read than a full AST walk.

use crate::model::Section;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    /// 0-based line index within the body.
    pub line: usize,
}

pub fn lines(body: &str) -> Vec<&str> {
    body.lines().collect()
}

/// ATX headings outside fenced code blocks.
pub fn headings(lines: &[&str]) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for (i, raw) in lines.iter().enumerate() {
        let t = raw.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(rest) = raw.strip_prefix('#') {
            let level = 1 + rest.chars().take_while(|c| *c == '#').count();
            if level > 6 {
                continue;
            }
            let text = rest.trim_start_matches('#');
            if !text.starts_with(' ') && !text.is_empty() {
                continue;
            }
            let text = text.trim().trim_end_matches('#').trim();
            out.push(Heading { level: level as u8, text: strip_inline(text), line: i });
        }
    }
    out
}

/// Remove the most common inline markup so headings/titles read as plain text.
pub fn strip_inline(s: &str) -> String {
    let mut t = s.replace("**", "").replace("__", "");
    t = t.replace('`', "");
    // [text](url) → text
    let mut out = String::new();
    let mut rest = t.as_str();
    while let Some(open) = rest.find('[') {
        if let Some(close) = rest[open..].find("](") {
            if let Some(end) = rest[open + close..].find(')') {
                out.push_str(&rest[..open]);
                out.push_str(&rest[open + 1..open + close]);
                rest = &rest[open + close + end + 1..];
                continue;
            }
        }
        break;
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// The first paragraph after the first H1 (or from the top when there is no H1): consecutive
/// non-empty lines that are not headings, lists, code fences or bold-label metadata lines.
pub fn first_paragraph(lines: &[&str], after_line: Option<usize>) -> Option<String> {
    let start = after_line.map(|l| l + 1).unwrap_or(0);
    let mut para: Vec<String> = Vec::new();
    let mut started = false;
    let mut in_fence = false;
    for raw in lines.iter().skip(start) {
        let t = raw.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            if started {
                break;
            }
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let is_meta = t.starts_with("**") && t.contains(":**");
        let is_block = t.starts_with('#')
            || t.starts_with("- ")
            || t.starts_with("* ")
            || t.starts_with('>')
            || t.starts_with('|')
            || t.starts_with("<!--")
            || is_meta
            || t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains(". ");
        if t.is_empty() {
            if started {
                break;
            }
            continue;
        }
        if is_block {
            if started {
                break;
            }
            continue;
        }
        started = true;
        para.push(strip_inline(t));
    }
    if para.is_empty() { None } else { Some(para.join(" ")) }
}

/// List items directly under the heading at `heading_line` (until the next heading of the
/// same or higher level). Nested items are joined to their parent with " — ".
pub fn list_items_under(lines: &[&str], heading: &Heading) -> Vec<(String, Option<bool>)> {
    let mut out: Vec<(String, Option<bool>)> = Vec::new();
    let mut in_fence = false;
    for raw in lines.iter().skip(heading.line + 1) {
        let t = raw.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(rest) = raw.strip_prefix('#') {
            let level = 1 + rest.chars().take_while(|c| *c == '#').count();
            if level as u8 <= heading.level {
                break;
            }
            continue;
        }
        let indent = raw.len() - t.len();
        let item = if let Some(r) =
            t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ "))
        {
            Some(r)
        } else {
            let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")) {
                Some(&t[digits + 2..])
            } else {
                None
            }
        };
        let Some(item) = item else { continue };
        let (checked, text) = if let Some(r) = item.strip_prefix("[ ] ") {
            (Some(false), r)
        } else if let Some(r) = item.strip_prefix("[x] ").or_else(|| item.strip_prefix("[X] ")) {
            (Some(true), r)
        } else {
            (None, item)
        };
        let text = strip_inline(text);
        if indent >= 2 && !out.is_empty() {
            let last = out.last_mut().unwrap();
            last.0 = format!("{} — {}", last.0, text);
        } else {
            out.push((text, checked));
        }
    }
    out
}

/// Sections for every H2/H3: the body runs until the next heading of the same or a higher
/// level.
pub fn sections(lines: &[&str], heads: &[Heading]) -> Vec<Section> {
    let mut out = Vec::new();
    for (i, h) in heads.iter().enumerate() {
        if h.level < 2 || h.level > 3 {
            continue;
        }
        let end = heads[i + 1..]
            .iter()
            .find(|n| n.level <= h.level)
            .map(|n| n.line)
            .unwrap_or(lines.len());
        let body = lines[h.line + 1..end].join("\n").trim().to_string();
        out.push(Section { level: h.level, heading: h.text.clone(), body_md: body });
    }
    out
}

/// Normalise a heading for matching: lowercase, alphanumerics and spaces only.
pub fn norm_heading(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Search\n\nAllows users to search **invoices**, clients, and projects.\nSecond line.\n\n```\n# not a heading\n```\n\n## Acceptance criteria\n\n- Results appear within 200 ms\n- [x] Tolerates typos\n  - nested detail\n1. Respects permissions\n\n## Notes\n\ntext\n### Sub\nmore\n";

    #[test]
    fn outline_and_paragraph() {
        let l = lines(DOC);
        let h = headings(&l);
        assert_eq!(
            h.iter().map(|h| (h.level, h.text.as_str())).collect::<Vec<_>>(),
            vec![(1, "Search"), (2, "Acceptance criteria"), (2, "Notes"), (3, "Sub")]
        );
        assert_eq!(
            first_paragraph(&l, Some(0)).unwrap(),
            "Allows users to search invoices, clients, and projects. Second line."
        );
        let ac = list_items_under(&l, &h[1]);
        assert_eq!(ac.len(), 3);
        assert_eq!(ac[1], ("Tolerates typos — nested detail".to_string(), Some(true)));
        assert_eq!(ac[2].0, "Respects permissions");
        let s = sections(&l, &h);
        assert_eq!(s.len(), 3);
        assert_eq!(s[1].heading, "Notes");
        assert_eq!(s[1].body_md, "text\n### Sub\nmore");
        assert_eq!(norm_heading("Acceptance-Criteria:"), "acceptance criteria");
        assert_eq!(strip_inline("[ADR-0022](adr/0022-x.md) and `code`"), "ADR-0022 and code");
    }
}
