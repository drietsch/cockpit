//! Terminal output helpers shared by every command: status language, colour, JSON envelopes.

use crate::model::HealthState;

#[derive(Clone, Copy, Debug)]
pub struct Format {
    pub json: bool,
    pub color: bool,
    pub ascii: bool,
}

pub struct Glyphs;

impl Format {
    pub fn glyph(&self, state: HealthState) -> String {
        let (g, word, color) = match state {
            HealthState::Healthy => (if self.ascii { "OK" } else { "✓" }, "Healthy", "32"),
            HealthState::Attention => {
                (if self.ascii { "WARN" } else { "⚠" }, "Needs attention", "33")
            }
            HealthState::Broken => (if self.ascii { "FAIL" } else { "✕" }, "Broken", "31"),
            HealthState::Unknown => (if self.ascii { "?" } else { "?" }, "Unknown", "90"),
        };
        self.paint(color, &format!("{g} {word}"))
    }

    pub fn icon(&self, state: HealthState) -> String {
        let (g, color) = match state {
            HealthState::Healthy => (if self.ascii { "OK" } else { "✓" }, "32"),
            HealthState::Attention => (if self.ascii { "WARN" } else { "⚠" }, "33"),
            HealthState::Broken => (if self.ascii { "FAIL" } else { "✕" }, "31"),
            HealthState::Unknown => ("?", "90"),
        };
        self.paint(color, g)
    }

    pub fn stale(&self) -> String {
        self.paint("33", if self.ascii { "STALE" } else { "↻" })
    }

    pub fn paint(&self, code: &str, s: &str) -> String {
        if self.color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }

    pub fn dim(&self, s: &str) -> String {
        self.paint("2", s)
    }

    pub fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }

    pub fn error(&self, code: i32, kind: &str, message: &str) {
        if self.json {
            println!(
                "{}",
                serde_json::json!({ "schema": "cockpit.error.v1", "code": code, "kind": kind, "message": message })
            );
        } else {
            eprintln!("{} {message}", self.paint("31", "error:"));
        }
    }

    pub fn json_out<T: serde::Serialize>(
        &self,
        schema: &str,
        project: &crate::model::Project,
        data: T,
    ) {
        let doc = serde_json::json!({
            "schema": schema,
            "generated_at": crate::time::now().to_string(),
            "project": { "id": project.id, "name": project.name, "root": project.root },
            "data": data,
        });
        println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
    }
}

pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
