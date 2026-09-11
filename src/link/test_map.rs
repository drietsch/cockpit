//! `[[test_map]]` entries: declarative test → feature mapping for suites whose tests carry no
//! tags yet. Files may be YAML or TOML; the inline TOML entries are appended.

use crate::config::{Located, TestMapEntry};
use crate::model::Test;
use regex::Regex;
use std::sync::LazyLock;

static AC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Z]{2,}(?:-[A-Z0-9]+)*-\d+\b").unwrap());

pub fn load(located: &Located) -> (Vec<TestMapEntry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let base = located
        .config_path
        .as_ref()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| located.root.clone());
    for f in &located.config.test_map.files {
        let path = base.join(f);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                warnings.push(format!("test map {}: {e}", path.display()));
                continue;
            }
        };
        let parsed: Result<Vec<TestMapEntry>, String> = if f.ends_with(".toml") {
            #[derive(serde::Deserialize)]
            struct Doc {
                #[serde(default, rename = "entry")]
                entries: Vec<TestMapEntry>,
            }
            toml::from_str::<Doc>(&text).map(|d| d.entries).map_err(|e| e.to_string())
        } else {
            serde_yaml_ng::from_str::<Vec<TestMapEntry>>(&text).map_err(|e| e.to_string())
        };
        match parsed {
            Ok(v) => entries.extend(v),
            Err(e) => warnings.push(format!("test map {}: {e}", path.display())),
        }
    }
    entries.extend(located.config.test_map.entries.iter().cloned());
    (entries, warnings)
}

/// Does an entry match a test? Every present key must match (AND); `file`/`title`/`classname`
/// are substrings, `ac_prefix` is a prefix of an acceptance-criterion code in the name.
pub fn matches(entry: &TestMapEntry, test: &Test, source_id: &str, suite_name: &str) -> bool {
    let m = &entry.matcher;
    if let Some(s) = &m.source {
        if s != source_id && s != suite_name && !source_id.starts_with(&format!("{s}/")) {
            return false;
        }
    }
    if let Some(s) = &m.suite {
        if s != suite_name && !source_id.ends_with(&format!("/{s}")) && source_id != s {
            return false;
        }
    }
    if let Some(f) = &m.file {
        let hay = test.source_path.as_ref().map(|p| p.as_str().to_string()).unwrap_or_default();
        let cls = test.classname.clone().unwrap_or_default();
        if !hay.contains(f.as_str()) && !cls.contains(f.as_str()) && !test.key.contains(f.as_str())
        {
            return false;
        }
    }
    if let Some(t) = &m.title {
        if !test.name.contains(t.as_str()) && !test.key.contains(t.as_str()) {
            return false;
        }
    }
    if let Some(c) = &m.classname {
        if !test.classname.as_deref().unwrap_or("").contains(c.as_str()) {
            return false;
        }
    }
    if let Some(p) = &m.ac_prefix {
        if !AC_RE.find_iter(&test.name).any(|code| code.as_str().starts_with(p.as_str())) {
            return false;
        }
    }
    m.source.is_some()
        || m.suite.is_some()
        || m.file.is_some()
        || m.title.is_some()
        || m.classname.is_some()
        || m.ac_prefix.is_some()
}
