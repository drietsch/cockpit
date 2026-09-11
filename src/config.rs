//! `cockpit.toml` — the (mostly optional) project configuration, plus discovery of the
//! project root and convention-based defaults when no file exists.
//!
//! Naming is fixed here and used everywhere else: an evidence source has a `format` (which
//! importer reads it), a `kind` (which evidence category it feeds: unit, e2e, visual …) and a
//! `path` (glob or list of globs, project-root-relative).

use crate::time::parse_span;
use jiff::SignedDuration;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error(
        "no cockpit.toml (or cockpit/cockpit.toml) and no .git found from {0} upward; run `cockpit init` or pass --project"
    )]
    NotFound(PathBuf),
    #[error("cannot read {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("invalid {path}: {message}")]
    Invalid { path: PathBuf, message: String },
}

/// Accepts either a single string or a list of strings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum OneOrMany {
    #[default]
    None,
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn items(&self) -> Vec<String> {
        match self {
            OneOrMany::None => vec![],
            OneOrMany::One(s) => vec![s.clone()],
            OneOrMany::Many(v) => v.clone(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.items().is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub project: ProjectSection,
    #[serde(rename = "repos")]
    pub repos: Vec<RepoSpec>,
    pub features: FeaturesSection,
    pub adrs: AdrsSection,
    #[serde(rename = "components")]
    pub components: Vec<ComponentSpec>,
    pub git: GitSection,
    #[serde(rename = "evidence")]
    pub evidence: Vec<EvidenceSource>,
    pub test_map: TestMapSection,
    pub health: HealthSection,
    pub performance: PerformanceSection,
    pub policy: PolicySection,
    pub scan: ScanSection,
    pub server: ServerSection,
    pub pull: PullSection,
    pub commits: CommitsSection,
    pub markdown: MarkdownSection,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectSection {
    pub id: Option<String>,
    pub name: Option<String>,
    /// Project root relative to the config file (`".."` for an umbrella folder).
    pub root: String,
    pub primary_branch: String,
}

impl Default for ProjectSection {
    fn default() -> Self {
        ProjectSection { id: None, name: None, root: ".".into(), primary_branch: "main".into() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct RepoSpec {
    pub id: Option<String>,
    pub path: String,
    pub primary_branch: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FeaturesSection {
    pub path: String,
    pub id_pattern: Option<String>,
    pub default_importance: String,
    pub default_lifecycle: String,
}

impl Default for FeaturesSection {
    fn default() -> Self {
        FeaturesSection {
            path: "docs/features".into(),
            id_pattern: None,
            default_importance: "normal".into(),
            default_lifecycle: "unspecified".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AdrsSection {
    pub path: OneOrMany,
    pub exclude: Vec<String>,
}

impl Default for AdrsSection {
    fn default() -> Self {
        AdrsSection {
            path: OneOrMany::One("docs/adr".into()),
            exclude: vec![
                "README.md".into(),
                "index.md".into(),
                "template.md".into(),
                "0000-template.md".into(),
            ],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ComponentSpec {
    pub id: String,
    pub title: Option<String>,
    pub group: Option<String>,
    pub order: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GitSection {
    pub implementation_paths_exclude: Vec<String>,
    pub max_commits: usize,
    pub dirty_check: bool,
    /// Auto-discovered repositories to leave out (path globs relative to the root).
    pub skip_repos: Vec<String>,
}

impl Default for GitSection {
    fn default() -> Self {
        GitSection {
            implementation_paths_exclude: vec![
                "docs/**".into(),
                "**/*.md".into(),
                "**/__screenshots__/**".into(),
                ".cockpit/**".into(),
                "artifacts/**".into(),
                "perf/results/**".into(),
            ],
            max_commits: 20_000,
            dirty_check: true,
            skip_repos: vec![],
        }
    }
}

/// Which importer reads a source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceFormat {
    Auto,
    Junit,
    Playwright,
    CockpitResults,
    PagedResults,
    CockpitBenchmark,
    GuardianBench,
    PagedMeasurements,
    Criterion,
    Fidelity,
    CockpitVisual,
}

impl EvidenceFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            EvidenceFormat::Auto => "auto",
            EvidenceFormat::Junit => "junit",
            EvidenceFormat::Playwright => "playwright",
            EvidenceFormat::CockpitResults => "cockpit-results",
            EvidenceFormat::PagedResults => "paged-results",
            EvidenceFormat::CockpitBenchmark => "cockpit-benchmark",
            EvidenceFormat::GuardianBench => "guardian-bench",
            EvidenceFormat::PagedMeasurements => "paged-measurements",
            EvidenceFormat::Criterion => "criterion",
            EvidenceFormat::Fidelity => "fidelity",
            EvidenceFormat::CockpitVisual => "cockpit-visual",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct GithubSource {
    pub repo: String,
    pub workflow: String,
    pub artifact: String,
    pub branch: Option<String>,
    pub runs_to_inspect: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SplitSpec {
    /// Cargo package names (nextest classname prefix) routed to the split source.
    pub packages: Vec<String>,
    pub id: String,
    pub component: Option<String>,
    pub suite: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ValiditySpec {
    pub treat_all_failed_as_invalid: bool,
    pub all_failed_min_tests: u32,
}

impl Default for ValiditySpec {
    fn default() -> Self {
        ValiditySpec { treat_all_failed_as_invalid: true, all_failed_min_tests: 20 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EvidenceSource {
    pub id: Option<String>,
    pub title: Option<String>,
    pub format: EvidenceFormat,
    /// Evidence category: unit | integration | e2e | contract | smoke | security | visual |
    /// benchmark | conformance. Defaults per format.
    pub kind: Option<String>,
    pub path: OneOrMany,
    pub suite: Option<String>,
    pub component: Option<String>,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub features: Vec<String>,
    pub enabled: bool,
    pub expect_every: Option<String>,
    pub allow_outside_root: bool,
    /// Conformance-style routing: per-result `level` → component.
    pub component_by_level: BTreeMap<String, String>,
    pub github: Option<GithubSource>,
    pub split: Vec<SplitSpec>,
    pub validity: ValiditySpec,
    /// Free-form note carried into `cockpit status`/`doctor` (e.g. why a lane is unfed).
    pub note: Option<String>,
}

impl Default for EvidenceSource {
    fn default() -> Self {
        EvidenceSource {
            id: None,
            title: None,
            format: EvidenceFormat::Auto,
            kind: None,
            path: OneOrMany::None,
            suite: None,
            component: None,
            repo: None,
            branch: None,
            features: vec![],
            enabled: true,
            expect_every: None,
            allow_outside_root: false,
            component_by_level: BTreeMap::new(),
            github: None,
            split: vec![],
            validity: ValiditySpec::default(),
            note: None,
        }
    }
}

impl EvidenceSource {
    pub fn paths(&self) -> Vec<String> {
        self.path.items()
    }
    pub fn default_kind(&self) -> &str {
        if let Some(k) = &self.kind {
            return k;
        }
        match self.format {
            EvidenceFormat::Playwright => "e2e",
            EvidenceFormat::Fidelity | EvidenceFormat::CockpitVisual => "visual",
            EvidenceFormat::CockpitBenchmark
            | EvidenceFormat::GuardianBench
            | EvidenceFormat::PagedMeasurements
            | EvidenceFormat::Criterion => "benchmark",
            _ => "unit",
        }
    }
    pub fn expect_every(&self, defaults: &HealthSection) -> Option<SignedDuration> {
        if let Some(s) = &self.expect_every {
            return parse_span(s);
        }
        if self.github.is_some() { parse_span(&defaults.suite_silence) } else { None }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TestMapSection {
    pub files: Vec<String>,
    #[serde(rename = "entry")]
    pub entries: Vec<TestMapEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TestMapMatch {
    pub source: Option<String>,
    pub suite: Option<String>,
    pub file: Option<String>,
    pub title: Option<String>,
    pub classname: Option<String>,
    pub ac_prefix: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TestMapEntry {
    #[serde(rename = "match")]
    pub matcher: TestMapMatch,
    pub features: Vec<String>,
    #[serde(alias = "stage")]
    pub component: Option<String>,
    #[serde(alias = "coverage_level")]
    pub level: Option<String>,
    pub kind: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HealthSection {
    pub max_evidence_age: String,
    pub flaky_window: u32,
    pub flaky_failures: u32,
    pub broken_on_failure_kinds: Vec<String>,
    pub required_evidence: BTreeMap<String, Vec<String>>,
    pub suite_silence: String,
    pub duration_regression_pct: f64,
    pub duration_regression_min_ms: u64,
    pub stale_failure_is_attention: bool,
}

impl Default for HealthSection {
    fn default() -> Self {
        let mut required = BTreeMap::new();
        required.insert("critical".to_string(), vec!["unit".to_string(), "e2e".to_string()]);
        required.insert("high".to_string(), vec!["unit".to_string()]);
        required.insert("normal".to_string(), vec![]);
        required.insert("low".to_string(), vec![]);
        HealthSection {
            max_evidence_age: "7d".into(),
            flaky_window: 20,
            flaky_failures: 3,
            broken_on_failure_kinds: [
                "unit",
                "integration",
                "e2e",
                "contract",
                "smoke",
                "security",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            required_evidence: required,
            suite_silence: "48h".into(),
            duration_regression_pct: 30.0,
            duration_regression_min_ms: 500,
            stale_failure_is_attention: true,
        }
    }
}

impl HealthSection {
    pub fn max_evidence_age(&self) -> SignedDuration {
        parse_span(&self.max_evidence_age).unwrap_or(SignedDuration::from_secs(7 * 86_400))
    }
    pub fn required_for(&self, importance: &str) -> Vec<String> {
        self.required_evidence.get(importance).cloned().unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RegressionPolicy {
    pub noise_pct: f64,
    pub gate: bool,
    pub require_significance: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PerformancePolicies {
    pub deterministic: RegressionPolicy,
    pub semi_deterministic: RegressionPolicy,
    pub environment_sensitive: RegressionPolicy,
}

impl Default for PerformancePolicies {
    fn default() -> Self {
        PerformancePolicies {
            deterministic: RegressionPolicy {
                noise_pct: 0.0,
                gate: true,
                require_significance: false,
            },
            semi_deterministic: RegressionPolicy {
                noise_pct: 10.0,
                gate: true,
                require_significance: false,
            },
            environment_sensitive: RegressionPolicy {
                noise_pct: 25.0,
                gate: false,
                require_significance: false,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct BudgetSpec {
    pub max: Option<f64>,
    pub min: Option<f64>,
    pub max_relative_pct: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct MetricOverride {
    /// Glob over `<name>/<subject>/<metric>`.
    #[serde(rename = "match")]
    pub matcher: String,
    pub id: Option<String>,
    pub title: Option<String>,
    pub unit: Option<String>,
    pub determinism: Option<String>,
    pub direction: Option<String>,
    pub budget: Option<BudgetSpec>,
    pub gate: Option<String>,
    pub tolerance_pct: Option<f64>,
    pub subjects: Vec<String>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PerformanceSection {
    pub baseline: String,
    pub history_limit: usize,
    pub policy: PerformancePolicies,
    #[serde(rename = "metrics")]
    pub metrics: Vec<MetricOverride>,
}

impl Default for PerformanceSection {
    fn default() -> Self {
        PerformanceSection {
            baseline: "previous-comparable".into(),
            history_limit: 200,
            policy: PerformancePolicies::default(),
            metrics: vec![],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PolicySection {
    pub references: PolicyReferences,
    pub evidence: PolicyEvidence,
    pub performance: PolicyPerformance,
    pub claims: PolicyClaims,
    /// Per-check severity overrides: `EV003 = "ignore" | "warn" | "error"`.
    pub checks: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PolicyReferences {
    pub fail_on_broken_reference: bool,
}
impl Default for PolicyReferences {
    fn default() -> Self {
        PolicyReferences { fail_on_broken_reference: true }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PolicyEvidence {
    pub critical: Option<Vec<String>>,
    pub high: Option<Vec<String>>,
    pub normal: Option<Vec<String>>,
    pub low: Option<Vec<String>>,
    pub require_fresh: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[derive(Default)]
pub struct PolicyPerformance {
    pub require_comparable_baseline: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PolicyClaims {
    pub drift: String,
}
impl Default for PolicyClaims {
    fn default() -> Self {
        PolicyClaims { drift: "ignore".into() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScanSection {
    pub ignore: Vec<String>,
    pub follow_symlinks: bool,
    pub max_file_size: String,
}
impl Default for ScanSection {
    fn default() -> Self {
        ScanSection {
            ignore: vec![
                "**/node_modules/**".into(),
                "**/.git/**".into(),
                "**/.cockpit/index.sqlite*".into(),
            ],
            follow_symlinks: false,
            max_file_size: "64MB".into(),
        }
    }
}
impl ScanSection {
    pub fn max_file_size_bytes(&self) -> u64 {
        let s = self.max_file_size.trim().to_ascii_uppercase();
        let (num, mult) = if let Some(n) = s.strip_suffix("GB") {
            (n, 1u64 << 30)
        } else if let Some(n) = s.strip_suffix("MB") {
            (n, 1u64 << 20)
        } else if let Some(n) = s.strip_suffix("KB") {
            (n, 1u64 << 10)
        } else {
            (s.as_str(), 1u64)
        };
        num.trim().parse::<u64>().map(|n| n * mult).unwrap_or(64 << 20)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerSection {
    pub bind: String,
    pub port: u16,
    pub artifact_roots: Vec<String>,
}
impl Default for ServerSection {
    fn default() -> Self {
        ServerSection { bind: "127.0.0.1".into(), port: 4747, artifact_roots: vec![] }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PullSection {
    pub dir: String,
    pub max_artifact_bytes: u64,
    pub timeout_secs: u64,
}
impl Default for PullSection {
    fn default() -> Self {
        PullSection {
            dir: ".cockpit/evidence".into(),
            max_artifact_bytes: 512 << 20,
            timeout_secs: 120,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct CommitsSection {
    /// Extra id regexes surfaced (informationally) on the Changes page.
    pub id_patterns: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct MarkdownSection {
    pub allow_html: bool,
}

/// A located project: where the root is, where the config came from, the config itself.
#[derive(Clone, Debug)]
pub struct Located {
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
    pub config: Config,
    /// `.cockpit/` lives beside the config file (or in the repo root for implicit projects).
    pub state_dir: PathBuf,
    pub implicit: bool,
}

impl Located {
    pub fn project_id(&self) -> String {
        self.config.project.id.clone().unwrap_or_else(|| slug(&dir_name(&self.root)))
    }
    pub fn project_name(&self) -> String {
        self.config.project.name.clone().unwrap_or_else(|| self.project_id())
    }
    pub fn index_path(&self) -> PathBuf {
        self.state_dir.join("index.sqlite")
    }
    pub fn evidence_dir(&self) -> PathBuf {
        self.state_dir.parent().unwrap_or(&self.state_dir).join(&self.config.pull.dir)
    }

    /// Evidence patterns are project-root-relative, except that `.cockpit/…` (the pull dir)
    /// always means the `.cockpit` beside the config file — on an umbrella project that is
    /// `<config dir>/.cockpit`, not `<root>/.cockpit`.
    pub fn resolve_pattern(&self, pattern: &str) -> String {
        let p = pattern.trim_start_matches("./");
        if let Some(rest) = p.strip_prefix(".cockpit/") {
            let dir = self.state_dir.parent().unwrap_or(&self.state_dir);
            if let Some(rel) = crate::paths::relativize(&self.root, dir) {
                if !rel.as_str().is_empty() {
                    return format!("{}/.cockpit/{rest}", rel.as_str());
                }
            }
        }
        p.to_string()
    }
}

fn dir_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "project".into())
}

pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

impl Config {
    pub fn parse(text: &str, path: &Path) -> Result<Config, ConfigError> {
        toml::from_str(text)
            .map_err(|e| ConfigError::Invalid { path: path.to_path_buf(), message: e.to_string() })
    }

    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::Io { path: path.to_path_buf(), source: e })?;
        Config::parse(&text, path)
    }

    /// Locate a project starting at `start`, honouring an explicit `--project` override.
    pub fn discover(start: &Path, explicit: Option<&Path>) -> Result<Located, ConfigError> {
        if let Some(p) = explicit {
            let p = if p.is_absolute() { p.to_path_buf() } else { start.join(p) };
            if p.is_file() {
                return Located::from_config_file(&p);
            }
            for cand in [p.join("cockpit.toml"), p.join("cockpit").join("cockpit.toml")] {
                if cand.is_file() {
                    return Located::from_config_file(&cand);
                }
            }
            if p.join(".git").exists() {
                return Ok(Located::implicit(&p));
            }
            return Err(ConfigError::NotFound(p));
        }
        let mut dir = Some(start.to_path_buf());
        let mut first_git: Option<PathBuf> = None;
        while let Some(d) = dir {
            for cand in [d.join("cockpit.toml"), d.join("cockpit").join("cockpit.toml")] {
                if cand.is_file() {
                    return Located::from_config_file(&cand);
                }
            }
            if first_git.is_none() && d.join(".git").exists() {
                first_git = Some(d.clone());
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
        match first_git {
            Some(g) => Ok(Located::implicit(&g)),
            None => Err(ConfigError::NotFound(start.to_path_buf())),
        }
    }
}

impl Located {
    fn from_config_file(path: &Path) -> Result<Located, ConfigError> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let config = Config::load(&path)?;
        let config_dir =
            path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
        let root = config_dir.join(&config.project.root);
        let root = root.canonicalize().unwrap_or(root);
        Ok(Located {
            root,
            config_path: Some(path),
            config,
            state_dir: config_dir.join(".cockpit"),
            implicit: false,
        })
    }

    fn implicit(root: &Path) -> Located {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        Located {
            state_dir: root.join(".cockpit"),
            root,
            config_path: None,
            config: Config::default(),
            implicit: true,
        }
    }

    /// The evidence sources to scan: configured ones, or the convention set when none exist.
    pub fn evidence_sources(&self) -> Vec<EvidenceSource> {
        if !self.config.evidence.is_empty() {
            return self.config.evidence.clone();
        }
        conventional_evidence()
    }
}

/// Convention-based evidence sources used when `[[evidence]]` is absent.
pub fn conventional_evidence() -> Vec<EvidenceSource> {
    let src = |id: &str, format: EvidenceFormat, globs: &[&str]| EvidenceSource {
        id: Some(format!("auto:{id}")),
        format,
        path: OneOrMany::Many(globs.iter().map(|s| s.to_string()).collect()),
        ..EvidenceSource::default()
    };
    vec![
        src(
            "junit",
            EvidenceFormat::Junit,
            &[
                "**/target/nextest/*/junit.xml",
                "artifacts/tests/**/*.xml",
                "**/test-results/junit.xml",
            ],
        ),
        src(
            "playwright",
            EvidenceFormat::Playwright,
            &["**/test-results/results.json", "artifacts/playwright/results.json"],
        ),
        src(
            "cockpit",
            EvidenceFormat::Auto,
            &[".cockpit/evidence/**/*.json", "artifacts/cockpit/**/*.json"],
        ),
        src("guardian-bench", EvidenceFormat::GuardianBench, &["perf/results/*.json"]),
        src(
            "paged-measurements",
            EvidenceFormat::PagedMeasurements,
            &["**/paged-measurements-*.json"],
        ),
        src("paged-results", EvidenceFormat::PagedResults, &["**/paged-results*.json"]),
        src("criterion", EvidenceFormat::Criterion, &["**/target/criterion/**/new/estimates.json"]),
        src("fidelity", EvidenceFormat::Fidelity, &["**/gate.json"]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_example() {
        let text = r#"
[project]
id = "paged"
name = "Paged"
root = ".."

[[repos]]
id = "core"
path = "core"

[features]
path = "cockpit/docs/features"

[adrs]
path = "thoughts/docs/paged/adr"

[[components]]
id = "core.parser"
title = "Parser"
group = "core"

[[evidence]]
id = "core/nextest-unit"
format = "junit"
kind = "unit"
path = ["core/target/nextest/ci/junit.xml"]
component = "core.parser"
repo = "core"
[evidence.github]
repo = "paged-media/core"
workflow = "ci.yml"
artifact = "nextest-junit"
[[evidence.split]]
packages = ["paged-script"]
id = "core-script/nextest-canvas-wasm"
component = "core.canvas-wasm"

[test_map]
files = ["test-map.yaml"]
[[test_map.entry]]
match = { file = "e2e/gesture-cancel.spec.ts" }
features = ["editor-tools.gesture-lifecycle"]
stage = "editor.gesture"

[health]
max_evidence_age = "14d"

[[performance.metrics]]
match = "*/*/store calls*"
determinism = "deterministic"
budget = { max = 150 }

[policy.checks]
EV003 = "ignore"
"#;
        let c = Config::parse(text, Path::new("cockpit.toml")).unwrap();
        assert_eq!(c.repos[0].path, "core");
        assert_eq!(c.evidence[0].format, EvidenceFormat::Junit);
        assert_eq!(c.evidence[0].github.as_ref().unwrap().artifact, "nextest-junit");
        assert_eq!(c.evidence[0].split[0].packages, vec!["paged-script"]);
        assert_eq!(c.test_map.entries[0].component.as_deref(), Some("editor.gesture"));
        assert_eq!(c.health.max_evidence_age().as_secs(), 14 * 86_400);
        assert_eq!(c.performance.metrics[0].budget.as_ref().unwrap().max, Some(150.0));
        assert_eq!(c.policy.checks["EV003"], "ignore");
    }

    #[test]
    fn unknown_keys_fail_loudly() {
        let err =
            Config::parse("[features]\npth = \"x\"\n", Path::new("cockpit.toml")).unwrap_err();
        assert!(err.to_string().contains("pth"), "{err}");
    }

    #[test]
    fn slug_and_sizes() {
        assert_eq!(slug("Invoice App!"), "invoice-app");
        assert_eq!(ScanSection::default().max_file_size_bytes(), 64 << 20);
    }

    #[test]
    fn discovery_prefers_config_over_git_and_supports_cockpit_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let umbrella = tmp.path();
        std::fs::create_dir_all(umbrella.join("core/.git")).unwrap();
        std::fs::create_dir_all(umbrella.join("core/crates/x")).unwrap();
        std::fs::create_dir_all(umbrella.join("cockpit")).unwrap();
        std::fs::write(umbrella.join("cockpit/cockpit.toml"), "[project]\nroot = \"..\"\n")
            .unwrap();
        let loc = Config::discover(&umbrella.join("core/crates/x"), None).unwrap();
        assert_eq!(loc.root, umbrella.canonicalize().unwrap());
        assert!(loc.state_dir.ends_with("cockpit/.cockpit"));
        assert!(!loc.implicit);

        let tmp2 = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp2.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp2.path().join("src")).unwrap();
        let loc = Config::discover(&tmp2.path().join("src"), None).unwrap();
        assert!(loc.implicit);
        assert_eq!(loc.evidence_sources().len(), conventional_evidence().len());
    }
}
