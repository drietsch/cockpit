//! Time handling. Everything inside Cockpit is a UTC `jiff::Timestamp`; parsing accepts the
//! offset forms tools actually emit (nextest writes `+02:00`, GitHub writes `Z`).

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

/// Parse an RFC 3339 / ISO-8601 timestamp with an offset. Also accepts a bare
/// `YYYY-MM-DDTHH:MM:SS` (assumed UTC) and a bare date (`YYYY-MM-DD`, midnight UTC).
pub fn parse_timestamp(s: &str) -> Option<Timestamp> {
    let s = s.trim();
    if let Ok(t) = s.parse::<Timestamp>() {
        return Some(t);
    }
    if let Ok(dt) = s.parse::<jiff::civil::DateTime>() {
        return dt.to_zoned(jiff::tz::TimeZone::UTC).ok().map(|z| z.timestamp());
    }
    if let Ok(d) = s.parse::<jiff::civil::Date>() {
        return d.to_zoned(jiff::tz::TimeZone::UTC).ok().map(|z| z.timestamp());
    }
    None
}

pub fn parse_date(s: &str) -> Option<jiff::civil::Date> {
    s.trim().parse::<jiff::civil::Date>().ok()
}

/// Parse a human duration such as `250ms`, `30s`, `15m`, `48h`, `7d`, `2w`.
pub fn parse_span(s: &str) -> Option<SignedDuration> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit())?;
    let (num, unit) = s.split_at(split);
    let n: i64 = num.parse().ok()?;
    let secs = match unit.trim() {
        "ms" => return Some(SignedDuration::from_millis(n)),
        "s" | "sec" | "secs" => n,
        "m" | "min" | "mins" => n * 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => n * 3600,
        "d" | "day" | "days" => n * 86_400,
        "w" | "week" | "weeks" => n * 7 * 86_400,
        _ => return None,
    };
    Some(SignedDuration::from_secs(secs))
}

pub fn now() -> Timestamp {
    Timestamp::now()
}

pub fn millis(t: Timestamp) -> i64 {
    t.as_millisecond()
}

pub fn from_millis(ms: i64) -> Timestamp {
    Timestamp::from_millisecond(ms).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// Relative time in the compact form the UI and CLI share: `just now`, `4 min ago`, `3 h ago`,
/// `2 d ago`, `3 w ago`, or a date for anything older than ~8 weeks.
pub fn ago(t: Timestamp, now: Timestamp) -> String {
    let d = now.duration_since(t);
    let secs = d.as_secs();
    if secs < 0 {
        return "in the future".to_string();
    }
    match secs {
        0..=44 => "just now".into(),
        45..=89 => "1 min ago".into(),
        90..=3_599 => format!("{} min ago", (secs + 30) / 60),
        3_600..=86_399 => format!("{} h ago", (secs + 1_800) / 3_600),
        86_400..=1_209_599 => format!("{} d ago", (secs + 43_200) / 86_400),
        1_209_600..=4_838_399 => format!("{} w ago", (secs + 302_400) / 604_800),
        _ => {
            let z = t.to_zoned(jiff::tz::TimeZone::UTC);
            let same_year = z.year() == now.to_zoned(jiff::tz::TimeZone::UTC).year();
            if same_year {
                z.strftime("%b %-d").to_string()
            } else {
                z.strftime("%b %-d, %Y").to_string()
            }
        }
    }
}

/// Compact duration for elapsed spans: `14 min`, `3 h 2 min`, `2 d 4 h`.
pub fn span_compact(d: SignedDuration) -> String {
    let secs = d.as_secs().max(0);
    if secs < 60 {
        return format!("{secs} s");
    }
    if secs < 3_600 {
        return format!("{} min", secs / 60);
    }
    if secs < 86_400 {
        let h = secs / 3_600;
        let m = (secs % 3_600) / 60;
        return if m == 0 { format!("{h} h") } else { format!("{h} h {m} min") };
    }
    let dd = secs / 86_400;
    let h = (secs % 86_400) / 3_600;
    if h == 0 { format!("{dd} d") } else { format!("{dd} d {h} h") }
}

/// Where a timestamp (or commit) came from — declared by the producer, read from a
/// `provenance.json` sidecar, taken from file metadata, inferred, or defaulted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Declared,
    Sidecar,
    FileMtime,
    CommitDate,
    Inferred,
    Default,
    Config,
}

impl Origin {
    pub fn as_str(&self) -> &'static str {
        match self {
            Origin::Declared => "declared",
            Origin::Sidecar => "sidecar",
            Origin::FileMtime => "file_mtime",
            Origin::CommitDate => "commit_date",
            Origin::Inferred => "inferred",
            Origin::Default => "default",
            Origin::Config => "config",
        }
    }
    pub fn parse(s: &str) -> Origin {
        match s {
            "declared" => Origin::Declared,
            "sidecar" => Origin::Sidecar,
            "file_mtime" => Origin::FileMtime,
            "commit_date" => Origin::CommitDate,
            "inferred" => Origin::Inferred,
            "config" => Origin::Config,
            _ => Origin::Default,
        }
    }
}

/// A value together with where it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamped<T> {
    pub value: T,
    pub origin: Origin,
}

impl<T> Stamped<T> {
    pub fn new(value: T, origin: Origin) -> Self {
        Stamped { value, origin }
    }
    pub fn declared(value: T) -> Self {
        Stamped { value, origin: Origin::Declared }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_offsets_and_bare_forms() {
        let t = parse_timestamp("2026-09-09T14:14:51.502+02:00").unwrap();
        assert_eq!(t.to_string(), "2026-09-09T12:14:51.502Z");
        assert!(parse_timestamp("2026-06-20T09:54:49.615Z").is_some());
        assert!(parse_timestamp("2026-06-20T09:54:49").is_some());
        assert!(parse_timestamp("2026-06-20").is_some());
        assert!(parse_timestamp("nope").is_none());
    }

    #[test]
    fn spans() {
        assert_eq!(parse_span("7d").unwrap().as_secs(), 7 * 86_400);
        assert_eq!(parse_span("48h").unwrap().as_secs(), 48 * 3_600);
        assert_eq!(parse_span("250ms").unwrap().as_millis(), 250);
        assert!(parse_span("7 fortnights").is_none());
    }

    #[test]
    fn ago_buckets() {
        let now = parse_timestamp("2026-09-11T12:00:00Z").unwrap();
        let at = |s: &str| parse_timestamp(s).unwrap();
        assert_eq!(ago(at("2026-09-11T11:59:50Z"), now), "just now");
        assert_eq!(ago(at("2026-09-11T11:56:00Z"), now), "4 min ago");
        assert_eq!(ago(at("2026-09-11T09:00:00Z"), now), "3 h ago");
        assert_eq!(ago(at("2026-09-09T12:00:00Z"), now), "2 d ago");
        assert_eq!(ago(at("2026-08-21T12:00:00Z"), now), "3 w ago");
        assert_eq!(ago(at("2026-05-01T12:00:00Z"), now), "May 1");
        assert_eq!(ago(at("2025-05-01T12:00:00Z"), now), "May 1, 2025");
    }
}
