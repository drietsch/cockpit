//! Markdown → safe HTML (comrak, raw HTML dropped) with repository links rewritten to Cockpit
//! routes.

use comrak::nodes::NodeValue;
use comrak::{Arena, Options, format_html, parse_document};

pub struct LinkContext<'a> {
    /// Directory of the source document, project-root-relative.
    pub base_dir: &'a str,
    pub repo: &'a str,
    /// Known ADR source path → id.
    pub adr_paths: &'a std::collections::HashMap<String, String>,
    pub feature_paths: &'a std::collections::HashMap<String, String>,
}

pub fn render(md: &str, links: Option<&LinkContext>) -> String {
    let mut opts = Options::default();
    opts.extension.table = true;
    opts.extension.tasklist = true;
    opts.extension.strikethrough = true;
    opts.extension.autolink = true;
    opts.extension.footnotes = true;
    opts.extension.header_id_prefix = Some("h-".into());

    opts.extension.front_matter_delimiter = Some("---".into());
    opts.render.r#unsafe = false;
    opts.render.github_pre_lang = true;
    let arena = Arena::new();
    let root = parse_document(&arena, md, &opts);
    if let Some(lc) = links {
        for node in root.descendants() {
            let mut data = node.data.borrow_mut();
            match &mut data.value {
                NodeValue::Link(l) => l.url = rewrite(&l.url, lc, false),
                NodeValue::Image(l) => l.url = rewrite(&l.url, lc, true),
                _ => {}
            }
        }
    }
    let mut out = String::new();
    let _ = format_html(root, &opts, &mut out);
    out
}

fn rewrite(url: &str, lc: &LinkContext, image: bool) -> String {
    if url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with('#')
        || url.starts_with("mailto:")
        || url.starts_with('/')
    {
        return url.to_string();
    }
    let (path, anchor) = match url.split_once('#') {
        Some((p, a)) => (p, format!("#{a}")),
        None => (url, String::new()),
    };
    let joined = crate::paths::RelPath::new(format!("{}/{}", lc.base_dir, path));
    let key = joined.as_str();
    if let Some(id) = lc.adr_paths.get(key) {
        return format!("/adrs/{}{anchor}", super::view::urlencode(id));
    }
    if let Some(id) = lc.feature_paths.get(key) {
        return format!("/features/{}{anchor}", super::view::urlencode(id));
    }
    if image {
        return format!("/artifacts/{}/{}", lc.repo, key);
    }
    format!("/files/{}/{}{anchor}", lc.repo, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_and_rewrites() {
        let mut adr = std::collections::HashMap::new();
        adr.insert("docs/adr/0022-x.md".to_string(), "ADR-0022".to_string());
        let lc = LinkContext {
            base_dir: "docs/adr",
            repo: "root",
            adr_paths: &adr,
            feature_paths: &Default::default(),
        };
        let html = render(
            "See [ADR-0022](0022-x.md#h-decision) and [spec](../spec/gcf.md) <script>x</script>",
            Some(&lc),
        );
        assert!(html.contains("href=\"/adrs/ADR-0022#h-decision\""));
        assert!(html.contains("href=\"/files/root/docs/spec/gcf.md\""));
        assert!(!html.contains("<script>"));
    }
}
