//! Parsing of `git log` records produced with
//! `--format='%x1e%H%x1f%P%x1f%an%x1f%ae%x1f%aI%x1f%cI%x1f%s%x1f%b%x1f%D%x1f%(trailers:only,unfold)' --numstat -M`.

use super::{RawCommit, RawFile};
use crate::time::parse_timestamp;

pub const LOG_FORMAT: &str =
    "%x1e%H%x1f%P%x1f%an%x1f%ae%x1f%aI%x1f%cI%x1f%s%x1f%b%x1f%D%x1f%(trailers:only,unfold)";

pub fn parse_log(output: &str) -> Vec<RawCommit> {
    let mut out = Vec::new();
    for record in output.split('\x1e') {
        if record.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = record.splitn(10, '\x1f').collect();
        if fields.len() < 10 {
            continue;
        }
        let sha = fields[0].trim().to_string();
        let parents = fields[1].split_whitespace().map(|s| s.to_string()).collect();
        let author = fields[2].to_string();
        let author_email = fields[3].to_string();
        let authored_at = parse_timestamp(fields[4]).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
        let committed_at = parse_timestamp(fields[5]).unwrap_or(authored_at);
        let subject = fields[6].to_string();
        let body = fields[7].trim_end().to_string();
        let refs = fields[8]
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        let (trailers, files) = parse_tail(fields[9]);
        out.push(RawCommit {
            sha,
            parents,
            author,
            author_email,
            authored_at,
            committed_at,
            subject,
            body,
            refs,
            trailers,
            files,
        });
    }
    out
}

/// The last field holds the unfolded trailers, a blank line, then numstat lines.
fn parse_tail(tail: &str) -> (Vec<(String, String)>, Vec<RawFile>) {
    let mut trailers = Vec::new();
    let mut files = Vec::new();
    for line in tail.lines() {
        if let Some(f) = parse_numstat(line) {
            files.push(f);
        } else if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            if !k.is_empty() && !k.contains(' ') && !k.contains('\t') {
                trailers.push((k.to_string(), v.trim().to_string()));
            }
        }
    }
    (trailers, files)
}

fn parse_numstat(line: &str) -> Option<RawFile> {
    let mut parts = line.splitn(3, '\t');
    let a = parts.next()?.trim();
    let d = parts.next()?.trim();
    let path = parts.next()?;
    let num = |s: &str| if s == "-" { None } else { s.parse::<u32>().ok() };
    if a != "-" && a.parse::<u32>().is_err() {
        return None;
    }
    if d != "-" && d.parse::<u32>().is_err() {
        return None;
    }
    let binary = a == "-" && d == "-";
    let (path, rename_from) = split_rename(path);
    Some(RawFile { path, rename_from, added: num(a), deleted: num(d), binary })
}

/// `dir/{old => new}/file` or `old => new` → (new, Some(old)).
fn split_rename(p: &str) -> (String, Option<String>) {
    if let (Some(open), Some(close)) = (p.find('{'), p.find('}')) {
        if open < close {
            let inner = &p[open + 1..close];
            if let Some((old, new)) = inner.split_once(" => ") {
                let prefix = &p[..open];
                let suffix = &p[close + 1..];
                return (
                    format!("{prefix}{new}{suffix}").replace("//", "/"),
                    Some(format!("{prefix}{old}{suffix}").replace("//", "/")),
                );
            }
        }
    }
    if let Some((old, new)) = p.split_once(" => ") {
        return (new.to_string(), Some(old.to_string()));
    }
    (p.to_string(), None)
}

/// `git status --porcelain=v1 -z` → number of changed entries.
pub fn count_status_entries(output: &[u8]) -> u32 {
    output.split(|b| *b == 0).filter(|e| !e.is_empty()).count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_records_with_numstat_and_trailers() {
        let s = "\x1eaaaa\x1fbbbb\x1fDee\x1fd@x\x1f2026-09-11T11:53:58+02:00\x1f2026-09-11T11:53:58+02:00\x1fSubject here\x1fBody line 1\n\nCo-Authored-By: X <x@y>\n\x1fHEAD -> main, tag: v1.2.0\x1fCo-Authored-By: X <x@y>\nClaude-Session: https://s\n\n\n30\t1\tsrc/a.rs\n-\t-\timg.png\n3\t0\tdir/{old => new}/f.rs\n\x1ebbbb\x1f\x1fDee\x1fd@x\x1f2026-09-10T10:00:00Z\x1f2026-09-10T10:00:00Z\x1fRoot\x1f\x1f\x1f\n\n1\t1\tREADME.md\n";
        let c = parse_log(s);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].sha, "aaaa");
        assert_eq!(c[0].parents, vec!["bbbb"]);
        assert_eq!(c[0].subject, "Subject here");
        assert_eq!(c[0].refs, vec!["HEAD -> main", "tag: v1.2.0"]);
        assert_eq!(c[0].trailers.len(), 2);
        assert_eq!(c[0].trailers[1].0, "Claude-Session");
        assert_eq!(c[0].files.len(), 3);
        assert_eq!(c[0].files[0].path, "src/a.rs");
        assert_eq!(c[0].files[0].added, Some(30));
        assert!(c[0].files[1].binary);
        assert_eq!(c[0].files[2].path, "dir/new/f.rs");
        assert_eq!(c[0].files[2].rename_from.as_deref(), Some("dir/old/f.rs"));
        assert!(c[1].parents.is_empty());
        assert_eq!(c[1].files[0].path, "README.md");
        assert_eq!(c[0].committed_at.to_string(), "2026-09-11T09:53:58Z");
    }
}
