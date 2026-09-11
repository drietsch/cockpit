//! Content sniffing for `format = "auto"` sources and for `.json` files whose importer is
//! not known from the path alone.

use crate::config::EvidenceFormat;

pub fn sniff(bytes: &[u8], path: &str) -> Option<EvidenceFormat> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    let trimmed = head.trim_start();
    if trimmed.starts_with('<') {
        return if trimmed.contains("<testsuite") { Some(EvidenceFormat::Junit) } else { None };
    }
    if !trimmed.starts_with('{') {
        return None;
    }
    let schema = extract_string_field(&head, "schema");
    match schema.as_deref() {
        Some("cockpit.results.v1") => return Some(EvidenceFormat::CockpitResults),
        Some("cockpit.benchmark.v1") => return Some(EvidenceFormat::CockpitBenchmark),
        Some("cockpit.visual.v1") => return Some(EvidenceFormat::CockpitVisual),
        _ => {}
    }
    if head.contains("\"contract\"") {
        if head.contains("\"results\"") || path.contains("paged-results") {
            return Some(EvidenceFormat::PagedResults);
        }
        if head.contains("\"measurements\"") || head.contains("\"lane\"") {
            return Some(EvidenceFormat::PagedMeasurements);
        }
    }
    if head.contains("\"determinism\"") && head.contains("\"generator_version\"") {
        return Some(EvidenceFormat::GuardianBench);
    }
    if head.contains("\"suites\"") && head.contains("\"config\"") {
        return Some(EvidenceFormat::Playwright);
    }
    if path.ends_with("gate.json") {
        return Some(EvidenceFormat::Fidelity);
    }
    if path.ends_with("/new/estimates.json") {
        return Some(EvidenceFormat::Criterion);
    }
    None
}

fn extract_string_field(head: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let i = head.find(&needle)?;
    let rest = &head[i + needle.len()..];
    let colon = rest.find(':')?;
    let rest = rest[colon + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sniffs_known_shapes() {
        assert_eq!(
            sniff(b"{\"schema\": \"cockpit.results.v1\"}", "x.json"),
            Some(EvidenceFormat::CockpitResults)
        );
        assert_eq!(
            sniff(b"{\"contract\":1,\"source\":\"x\",\"results\":[]}", "x.json"),
            Some(EvidenceFormat::PagedResults)
        );
        assert_eq!(sniff(b"{\"name\":\"memory\",\"generator_version\":1,\"measurements\":[{\"determinism\":\"deterministic\"}]}", "m.json"), Some(EvidenceFormat::GuardianBench));
        assert_eq!(
            sniff(b"{\"config\":{},\"suites\":[]}", "results.json"),
            Some(EvidenceFormat::Playwright)
        );
        assert_eq!(
            sniff(b"<?xml version=\"1.0\"?><testsuites>", "j.xml"),
            Some(EvidenceFormat::Junit)
        );
        assert_eq!(sniff(b"hello", "x.txt"), None);
    }
}
