//! YAML frontmatter splitting: `---\n…\n---` (or `...`) at the very top of a Markdown file.

pub struct Split<'a> {
    pub yaml: Option<&'a str>,
    pub body: &'a str,
    /// 1-based line number where the body starts in the original text.
    pub body_line: usize,
}

pub fn split(text: &str) -> Split<'_> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return Split { yaml: None, body: text, body_line: 1 };
    };
    if first.trim_end() != "---" {
        return Split { yaml: None, body: text, body_line: 1 };
    }
    let mut offset = first.len();
    let yaml_start = offset;
    let mut line_no = 1usize;
    for line in lines {
        line_no += 1;
        let t = line.trim_end();
        if t == "---" || t == "..." {
            let yaml = &text[yaml_start..offset];
            let body_start = offset + line.len();
            return Split { yaml: Some(yaml), body: &text[body_start..], body_line: line_no + 1 };
        }
        offset += line.len();
    }
    // Unterminated frontmatter: treat the whole file as body.
    Split { yaml: None, body: text, body_line: 1 }
}

pub fn parse_yaml(yaml: &str) -> Result<serde_yaml_ng::Mapping, String> {
    match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(yaml) {
        Ok(serde_yaml_ng::Value::Mapping(m)) => Ok(m),
        Ok(serde_yaml_ng::Value::Null) => Ok(serde_yaml_ng::Mapping::new()),
        Ok(_) => Err("frontmatter is not a mapping".into()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_and_counts_lines() {
        let s = split("---\nid: X\ntitle: T\n---\n# H\n\nbody\n");
        assert_eq!(s.yaml.unwrap(), "id: X\ntitle: T\n");
        assert_eq!(s.body, "# H\n\nbody\n");
        assert_eq!(s.body_line, 5);
        let s = split("# no fm\n");
        assert!(s.yaml.is_none());
        assert_eq!(s.body_line, 1);
        let s = split("---\nunterminated\n");
        assert!(s.yaml.is_none());
    }
}
