//! Opt-in, metadata-only Claude Code transcript usage aggregation.
//! Transcript JSONL can contain prompts and secrets. Never return or persist it.
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const MAX_FILES: usize = 5_000;
const MAX_LINE: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

impl TokenUsage {
    fn from_json(u: &Value) -> Self {
        let field = |name| u.get(name).and_then(Value::as_u64).unwrap_or(0);
        Self {
            input: field("input_tokens"),
            output: field("output_tokens"),
            cache_read: field("cache_read_input_tokens"),
            cache_creation: field("cache_creation_input_tokens"),
        }
    }
    fn total(self) -> u64 {
        self.input.saturating_add(self.output).saturating_add(self.cache_read).saturating_add(self.cache_creation)
    }
    fn add(&mut self, other: Self) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_creation = self.cache_creation.saturating_add(other.cache_creation);
    }
}

#[derive(Debug, Serialize)]
pub struct ModelUsage {
    pub model: String,
    pub requests: usize,
    pub tokens: TokenUsage,
}

#[derive(Debug, Serialize)]
pub struct UsageSummary {
    pub source: &'static str,
    pub transcript_files_scanned: usize,
    pub matched_sessions: usize,
    pub requests: usize,
    pub tokens: TokenUsage,
    pub observed_tokens: u64,
    pub models: Vec<ModelUsage>,
    pub summed_session_span_seconds: u64,
    pub skipped_large_lines: u64,
    pub malformed_lines: u64,
    pub truncated: bool,
    pub estimated_cost_usd: Option<f64>,
    pub notes: Vec<&'static str>,
}

impl Default for UsageSummary {
    fn default() -> Self {
        Self {
            source: "local_claude_code_jsonl",
            transcript_files_scanned: 0,
            matched_sessions: 0,
            requests: 0,
            tokens: TokenUsage::default(),
            observed_tokens: 0,
            models: Vec::new(),
            summed_session_span_seconds: 0,
            skipped_large_lines: 0,
            malformed_lines: 0,
            truncated: false,
            estimated_cost_usd: None,
            notes: vec![
                "Read-only opt-in local transcript metadata; prompts, tool outputs and session IDs are not returned.",
                "Usage is deduplicated by assistant message id within each transcript.",
                "Observed span is first-to-last usage event, not active work time or model latency.",
                "Token totals sum usage categories; no cost or billing claims are inferred.",
                "Only assistant records whose cwd resolves inside the selected project are counted.",
            ],
        }
    }
}

#[derive(Clone)]
struct Request {
    model: String,
    usage: TokenUsage,
}
#[derive(Default)]
struct Session {
    messages: BTreeMap<String, Request>,
    first: Option<i64>,
    last: Option<i64>,
}

fn in_project(value: &Value, project: &Path) -> bool {
    let Some(cwd) = value.get("cwd").and_then(Value::as_str) else {
        return false;
    };
    Path::new(cwd).canonicalize().is_ok_and(|cwd| cwd.starts_with(project))
}

fn observe(value: &Value, project: &Path, session: &mut Session) {
    if value.get("type").and_then(Value::as_str) != Some("assistant") || !in_project(value, project) {
        return;
    }
    let Some(message) = value.get("message") else { return };
    let Some(id) = message.get("id").and_then(Value::as_str) else { return };
    let Some(usage) = message.get("usage") else { return };
    let usage = TokenUsage::from_json(usage);
    if usage.total() == 0 {
        return;
    }
    let model = message.get("model").and_then(Value::as_str).unwrap_or("unknown").to_owned();
    if let Some(time) = value.get("timestamp").and_then(Value::as_str)
        .and_then(|s| s.parse::<jiff::Timestamp>().ok())
        .map(|t| t.as_second())
    {
        session.first = Some(session.first.map_or(time, |old| old.min(time)));
        session.last = Some(session.last.map_or(time, |old| old.max(time)));
    }
    // A streaming response can appear several times with the same message id.
    if session.messages.get(id).is_none_or(|old| usage.total() >= old.usage.total()) {
        session.messages.insert(id.to_owned(), Request { model, usage });
    }
}

// Bounded line reader: malformed or very large prompt/tool lines are discarded
// without allocating their entire contents.
fn read_line_bounded(reader: &mut impl BufRead, buffer: &mut Vec<u8>) -> std::io::Result<(bool, bool)> {
    buffer.clear();
    let mut too_large = false;
    let mut saw_bytes = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok((saw_bytes, too_large));
        }
        saw_bytes = true;
        let n = available.iter().position(|b| *b == b'\n').map_or(available.len(), |n| n + 1);
        if !too_large && buffer.len().saturating_add(n) <= MAX_LINE {
            buffer.extend_from_slice(&available[..n]);
        } else {
            too_large = true;
            buffer.clear();
        }
        let ended = available[n - 1] == b'\n';
        reader.consume(n);
        if ended {
            return Ok((true, too_large));
        }
    }
}

fn scan_one(path: &Path, project: &Path, result: &mut UsageSummary) -> Result<Session> {
    let input = File::open(path).context("opening local Claude Code transcript")?;
    let mut reader = BufReader::new(input);
    let mut buffer = Vec::new();
    let mut session = Session::default();
    loop {
        let (more, too_large) = read_line_bounded(&mut reader, &mut buffer)?;
        if !more {
            break;
        }
        if too_large {
            result.skipped_large_lines += 1;
            continue;
        }
        match serde_json::from_slice::<Value>(&buffer) {
            Ok(v) => observe(&v, project, &mut session),
            Err(_) => result.malformed_lines += 1,
        }
    }
    Ok(session)
}

pub fn default_transcripts_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude/projects"))
}

/// This does not write files or contact Claude's servers.
pub fn summarize(project: &Path, transcripts_dir: &Path) -> Result<UsageSummary> {
    let project = project.canonicalize().context("resolving project root")?;
    let mut result = UsageSummary::default();
    if !transcripts_dir.is_dir() {
        return Ok(result);
    }
    let mut stack = vec![(transcripts_dir.to_path_buf(), 0usize)];
    let mut models = BTreeMap::<String, (usize, TokenUsage)>::new();
    while let Some((dir, depth)) = stack.pop() {
        let entries = fs::read_dir(dir).context("reading Claude Code transcript directory")?;
        for entry in entries {
            let entry = entry?;
            let typ = entry.file_type()?;
            // Never traverse symlinks or unbounded nested directories.
            if typ.is_symlink() {
                continue;
            }
            if typ.is_dir() {
                if depth < 4 {
                    stack.push((entry.path(), depth + 1));
                }
                continue;
            }
            if !typ.is_file() || entry.path().extension().is_none_or(|ext| ext != "jsonl") {
                continue;
            }
            if result.transcript_files_scanned >= MAX_FILES {
                result.truncated = true;
                break;
            }
            result.transcript_files_scanned += 1;
            let session = scan_one(&entry.path(), &project, &mut result)?;
            if session.messages.is_empty() {
                continue;
            }
            result.matched_sessions += 1;
            if let (Some(first), Some(last)) = (session.first, session.last) {
                result.summed_session_span_seconds =
                    result.summed_session_span_seconds.saturating_add(last.saturating_sub(first) as u64);
            }
            for request in session.messages.into_values() {
                result.requests += 1;
                result.tokens.add(request.usage);
                let bucket = models.entry(request.model).or_default();
                bucket.0 += 1;
                bucket.1.add(request.usage);
            }
        }
        if result.truncated {
            break;
        }
    }
    result.observed_tokens = result.tokens.total();
    result.models = models.into_iter().map(|(model, (requests, tokens))| ModelUsage {
        model, requests, tokens,
    }).collect();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn counts_only_project_usage_and_deduplicates_streaming() {
        let t = tempfile::tempdir().unwrap();
        let project = t.path().join("project");
        let other = t.path().join("other");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&other).unwrap();
        let transcripts = t.path().join("transcripts");
        fs::create_dir_all(&transcripts).unwrap();
        let mut file = File::create(transcripts.join("one.jsonl")).unwrap();
        for (cwd, id, input, output) in [
            (&project, "m1", 10, 5),
            (&project, "m1", 15, 7),
            (&other, "m2", 1000, 1000),
        ] {
            writeln!(file, "{}", serde_json::json!({
                "type": "assistant", "cwd": cwd,
                "timestamp": "2026-10-31T08:00:00Z",
                "message": {"id": id, "model": "claude-test", "usage": {
                    "input_tokens": input, "output_tokens": output,
                    "cache_read_input_tokens": 3, "cache_creation_input_tokens": 2,
                }},
                "content": "ignored private content"
            })).unwrap();
        }
        writeln!(file, "{}", serde_json::json!({"type": "user", "content": "secret"})).unwrap();
        let report = summarize(&project, &transcripts).unwrap();
        assert_eq!(report.matched_sessions, 1);
        assert_eq!(report.requests, 1);
        assert_eq!(report.tokens.input, 15);
        assert_eq!(report.tokens.output, 7);
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("m1"));
        assert!(!json.contains(project.to_string_lossy().as_ref()));
    }

    #[test]
    fn missing_transcripts_does_not_invent_usage() {
        let root = tempfile::tempdir().unwrap();
        let report = summarize(root.path(), &root.path().join("missing")).unwrap();
        assert_eq!(report.requests, 0);
        assert!(report.models.is_empty());
        assert_eq!(report.estimated_cost_usd, None);
    }
}
