//! Synthetic fixture projects: a temp git repo with commits at controlled dates plus
//! generated documents and evidence files, so end-to-end tests are deterministic.

#![allow(dead_code)]

use cockpit::cockpit::Cockpit;
use cockpit::config::{Config, Located};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct ProjectBuilder {
    pub dir: tempfile::TempDir,
    pub root: PathBuf,
    config_text: Option<String>,
    commit_no: u32,
}

impl Default for ProjectBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectBuilder {
    pub fn new() -> ProjectBuilder {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["init", "-q", "-b", "main"], "2026-01-01T00:00:00Z");
        ProjectBuilder { dir, root, config_text: None, commit_no: 0 }
    }

    pub fn config(mut self, toml: &str) -> Self {
        self.config_text = Some(toml.to_string());
        std::fs::write(self.root.join("cockpit.toml"), toml).unwrap();
        self
    }

    pub fn file(self, rel: &str, content: &str) -> Self {
        let p = self.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
        self
    }

    pub fn feature(
        self,
        id: &str,
        title: &str,
        paths: &[&str],
        extra_frontmatter: &str,
        body: &str,
    ) -> Self {
        let paths_yaml = paths.iter().map(|p| format!("  - {p}")).collect::<Vec<_>>().join("\n");
        let text = format!(
            "---\nid: {id}\ntitle: {title}\nstatus: stable\npaths:\n{paths_yaml}\n{extra_frontmatter}---\n# {title}\n\n{body}\n"
        );
        self.file(&format!("docs/features/{id}.md"), &text)
    }

    pub fn adr(self, number: u32, title: &str, status: &str, date: &str, body: &str) -> Self {
        let text = format!(
            "# ADR-{number:04} — {title}\n\n**Status:** {status}\n**Date:** {date}\n\n## Context\n\n{body}\n\n## Decision\n\nDo it.\n"
        );
        self.file(&format!("docs/adr/{number:04}-{}.md", slug(title)), &text)
    }

    /// Commit everything currently in the tree with the given subject at the given time.
    pub fn commit(mut self, subject: &str, at: &str) -> Self {
        self.commit_no += 1;
        git(&self.root, &["add", "-A"], at);
        git(&self.root, &["commit", "-q", "--allow-empty", "-m", subject], at);
        self
    }

    pub fn commit_touching(self, subject: &str, at: &str, files: &[(&str, &str)]) -> Self {
        let mut b = self;
        for (p, c) in files {
            b = b.file(p, c);
        }
        b.commit(subject, at)
    }

    pub fn tag(self, name: &str) -> Self {
        git(&self.root, &["tag", name], "2026-01-01T00:00:00Z");
        self
    }

    pub fn head_sha(&self) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A nextest-style JUnit file.
    pub fn junit(self, rel: &str, report: &str, cases: &[JunitCase], timestamp: &str) -> Self {
        let mut suites: std::collections::BTreeMap<&str, Vec<&JunitCase>> = Default::default();
        for c in cases {
            suites.entry(c.classname).or_default().push(c);
        }
        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites name=\"{report}\" tests=\"{}\" failures=\"0\" errors=\"0\" timestamp=\"{timestamp}\" time=\"1.0\">\n",
            cases.len()
        );
        for (cls, cs) in suites {
            xml.push_str(&format!("  <testsuite name=\"{cls}\" tests=\"{}\" disabled=\"0\" errors=\"0\" failures=\"0\">\n", cs.len()));
            for c in cs {
                xml.push_str(&format!("    <testcase name=\"{}\" classname=\"{cls}\" timestamp=\"{timestamp}\" time=\"{}\">", c.name, c.secs));
                match c.status {
                    "failed" => xml
                        .push_str("<failure message=\"assertion failed\">left != right</failure>"),
                    "skipped" => xml.push_str("<skipped message=\"ignored\"/>"),
                    "flaky" => xml.push_str(
                        "<flakyFailure message=\"first attempt failed\">retry</flakyFailure>",
                    ),
                    _ => {}
                }
                xml.push_str("</testcase>\n");
            }
            xml.push_str("  </testsuite>\n");
        }
        xml.push_str("</testsuites>\n");
        self.file(rel, &xml)
    }

    pub fn located(&self) -> Located {
        Config::discover(&self.root, None).unwrap()
    }

    pub fn open(&self) -> Cockpit {
        Cockpit::from_located(self.located(), None).unwrap()
    }

    /// Full scan, returns the cockpit with a fresh state.
    pub fn scan(&self) -> Cockpit {
        let mut c = self.open();
        c.scan(None).unwrap();
        c
    }
}

pub struct JunitCase {
    pub classname: &'static str,
    pub name: &'static str,
    pub status: &'static str,
    pub secs: f64,
}

pub fn case(classname: &'static str, name: &'static str, status: &'static str) -> JunitCase {
    JunitCase { classname, name, status, secs: 0.01 }
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

pub fn git(dir: &Path, args: &[&str], at: &str) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.com")
        .env("GIT_AUTHOR_DATE", at)
        .env("GIT_COMMITTER_DATE", at)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?} failed");
}
