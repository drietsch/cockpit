//! Template filters and functions: relative time, units, status language, URLs.

use crate::model::HealthState;
use crate::time::{ago as ago_fmt, now, parse_timestamp};
use minijinja::{Environment, Error, ErrorKind, Value};

pub fn register(env: &mut Environment<'static>) {
    env.add_filter("ago", ago);
    env.add_filter("datetime", datetime);
    env.add_filter("short_sha", |s: String| s.chars().take(7).collect::<String>());
    env.add_filter("number", |n: f64| thousands(n as i64));
    env.add_filter("duration", duration);
    env.add_filter("pct", pct);
    env.add_filter("plural", |n: i64, one: String, many: String| if n == 1 { one } else { many });
    env.add_filter("markdown", markdown);
    env.add_filter("truncate_chars", |s: String, n: usize| {
        if s.chars().count() <= n {
            s
        } else {
            format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>())
        }
    });
    env.add_filter("json_script", json_script);
    env.add_function("status_label", status_label);
    env.add_function("status_icon", status_icon);
    env.add_function("url_for", url_for);
    env.add_function("asset", |p: String| format!("/assets/{p}"));
    env.add_function("now_iso", || now().to_string());
}

fn ago(v: Value) -> Result<String, Error> {
    let Some(s) = v.as_str() else { return Ok(String::new()) };
    match parse_timestamp(s) {
        Some(t) => Ok(ago_fmt(t, now())),
        None => Ok(s.to_string()),
    }
}

fn datetime(v: Value, fmt: Option<String>) -> Result<String, Error> {
    let Some(s) = v.as_str() else { return Ok(String::new()) };
    let Some(t) = parse_timestamp(s) else { return Ok(s.to_string()) };
    let z = t.to_zoned(jiff::tz::TimeZone::system());
    let f = match fmt.as_deref() {
        Some("time") => "%H:%M",
        Some("date") => "%b %-d, %Y",
        Some("long") => "%Y-%m-%d %H:%M:%S %Z",
        _ => "%Y-%m-%d %H:%M",
    };
    Ok(z.strftime(f).to_string())
}

pub fn thousands(n: i64) -> String {
    let neg = n < 0;
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if neg { format!("−{out}") } else { out }
}

/// Milliseconds → `8 ms`, `132 ms`, `1.82 s`, `2 min 14 s`.
fn duration(v: Value) -> Result<String, Error> {
    let ms = v
        .as_i64()
        .map(|i| i as f64)
        .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
        .unwrap_or(0.0);
    Ok(duration_ms(ms))
}

pub fn duration_ms(ms: f64) -> String {
    if ms < 1.0 {
        format!("{:.0} µs", ms * 1000.0)
    } else if ms < 1000.0 {
        format!("{:.0} ms", ms)
    } else if ms < 60_000.0 {
        format!("{:.2} s", ms / 1000.0)
    } else if ms < 3_600_000.0 {
        let s = (ms / 1000.0) as u64;
        format!("{} min {} s", s / 60, s % 60)
    } else {
        let s = (ms / 1000.0) as u64;
        format!("{} h {} min", s / 3600, (s % 3600) / 60)
    }
}

fn pct(v: f64, digits: Option<usize>) -> String {
    let d = digits.unwrap_or(1);
    let sign = if v > 0.0 {
        "+"
    } else if v < 0.0 {
        "−"
    } else {
        ""
    };
    format!("{sign}{:.*}%", d, (v * 100.0).abs())
}

/// JSON for a `<script type="application/json">` block: `</` can never close the element.
fn json_script(v: Value) -> Value {
    let raw = match v.as_str() {
        Some(s) => s.to_string(),
        None => serde_json::to_value(&v).map(|j| j.to_string()).unwrap_or_else(|_| "null".into()),
    };
    Value::from_safe_string(raw.replace("</", "<\\/").replace("<!--", "<\\!--"))
}

fn markdown(s: String) -> Value {
    Value::from_safe_string(super::markdown::render(&s, None))
}

fn status_label(state: String) -> String {
    match HealthState::parse(&state) {
        Some(s) => s.label().to_string(),
        None => match state.as_str() {
            "pass" => "Passing".into(),
            "fail" => "Failing".into(),
            "flaky" => "Flaky".into(),
            "stale" | "stale_fail" => "Stale".into(),
            "missing" => "No evidence".into(),
            "silent" => "Silent".into(),
            "regression" => "Regression".into(),
            "budget_breach" => "Budget breach".into(),
            "incomparable" => "Incomparable".into(),
            "improved" => "Improved".into(),
            "reported" => "Reported".into(),
            "stable" => "Stable".into(),
            "first_run" => "First run".into(),
            "accepted" => "Accepted".into(),
            "proposed" => "Proposed".into(),
            "draft" => "Draft".into(),
            "superseded" => "Superseded".into(),
            "rejected" => "Rejected".into(),
            "deprecated" => "Deprecated".into(),
            "current" => "Current".into(),
            "passed" => "Passed".into(),
            "failed" => "Failed".into(),
            "skipped" => "Skipped".into(),
            other => other.to_string(),
        },
    }
}

/// The icon id + css class for any status word (health, rollup, adr, test).
fn status_icon(state: String) -> Value {
    let (icon, class) = match state.as_str() {
        "healthy" | "pass" | "passed" | "current" | "accepted" | "improved" | "ok" | "stable" => {
            ("check", "ok")
        }
        "attention" | "flaky" | "stale" | "regression" | "warning" | "stale_fail" => {
            ("warn", "warn")
        }
        "reported" => ("info", "sec"),
        "change" => ("commit", "sec"),
        "broken" | "fail" | "failed" | "budget_breach" | "rejected" | "error" => ("x", "bad"),
        "unknown" | "incomparable" | "first_run" => ("help", "unk"),
        "missing" | "silent" | "skipped" => ("minus", "mut"),
        "proposed" | "draft" => ("circle", "sec"),
        "superseded" | "deprecated" => ("minus", "mut"),
        _ => ("minus", "mut"),
    };
    Value::from_serialize(serde_json::json!({ "icon": icon, "class": class }))
}

fn url_for(kind: String, id: String, repo: Option<String>) -> Result<String, Error> {
    let enc = |s: &str| urlencode(s);
    Ok(match kind.as_str() {
        "feature" => format!("/features/{}", enc(&id)),
        "adr" => format!("/adrs/{}", enc(&id)),
        "test" => format!("/tests/{}", enc(&id)),
        "benchmark" => format!("/performance/{}", enc(&id)),
        "visual" => format!("/visual/{}", enc(&id)),
        "commit" => match repo {
            Some(r) => format!("/commits/{}?repo={}", enc(&id), enc(&r)),
            None => format!("/commits/{}", enc(&id)),
        },
        "suite" => format!("/tests?suite={}", enc(&id)),
        "file" => match repo {
            Some(r) => format!("/files/{}/{}", enc(&r), id),
            None => format!("/files/root/{id}"),
        },
        other => {
            return Err(Error::new(
                ErrorKind::InvalidOperation,
                format!("url_for: unknown kind {other}"),
            ));
        }
    })
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
