//! GitHub Actions access: through the authenticated `gh` CLI when available, else with a token
//! (`GITHUB_TOKEN` / `GH_TOKEN`) over HTTPS. Tokens are never logged or written anywhere.

use serde::Deserialize;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Debug, Deserialize)]
pub struct RunInfo {
    pub id: u64,
    pub head_sha: String,
    #[serde(default)]
    pub head_branch: Option<String>,
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub run_attempt: Option<u32>,
    #[serde(default)]
    pub run_started_at: Option<String>,
    pub updated_at: String,
    pub html_url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ArtifactInfo {
    pub id: u64,
    pub name: String,
    pub size_in_bytes: u64,
    #[serde(default)]
    pub expired: bool,
    #[serde(default)]
    pub archive_download_url: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Deserialize)]
struct RunsPage {
    workflow_runs: Vec<RunInfo>,
}

#[derive(Deserialize)]
struct ArtifactsPage {
    artifacts: Vec<ArtifactInfo>,
}

pub trait GithubApi: Send + Sync {
    fn describe(&self) -> String;
    fn list_runs(
        &self,
        repo: &str,
        workflow: &str,
        branch: &str,
        per_page: u32,
    ) -> anyhow::Result<Vec<RunInfo>>;
    fn list_artifacts(&self, repo: &str, run_id: u64) -> anyhow::Result<Vec<ArtifactInfo>>;
    fn download_artifact(&self, repo: &str, art: &ArtifactInfo, dest: &Path) -> anyhow::Result<()>;
}

/// `gh` when logged in, else a token client, else nothing.
pub fn detect() -> Option<Box<dyn GithubApi>> {
    if let Ok(out) = Command::new("gh").args(["auth", "status"]).output() {
        if out.status.success() {
            return Some(Box::new(GhCli));
        }
    }
    let token = std::env::var("GITHUB_TOKEN")
        .ok()
        .or_else(|| std::env::var("GH_TOKEN").ok())
        .filter(|t| !t.trim().is_empty());
    token.map(|t| Box::new(TokenHttp { token: t }) as Box<dyn GithubApi>)
}

fn with_retry<T>(mut f: impl FnMut() -> anyhow::Result<T>) -> anyhow::Result<T> {
    let mut last = None;
    for attempt in 0..3u32 {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                let msg = e.to_string();
                let retryable = msg.contains("429")
                    || msg.contains("50") && msg.contains("HTTP")
                    || msg.to_lowercase().contains("timed out")
                    || msg.to_lowercase().contains("connection");
                last = Some(e);
                if !retryable {
                    break;
                }
                let wait = std::time::Duration::from_millis(
                    800 * (1 << attempt) + (attempt as u64 * 137) % 400,
                );
                std::thread::sleep(wait.min(std::time::Duration::from_secs(60)));
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("request failed")))
}

pub struct GhCli;

impl GhCli {
    fn api(&self, path: &str) -> anyhow::Result<Vec<u8>> {
        with_retry(|| {
            let out = Command::new("gh").args(["api", path]).output()?;
            if !out.status.success() {
                anyhow::bail!("gh api {path}: {}", String::from_utf8_lossy(&out.stderr).trim());
            }
            Ok(out.stdout)
        })
    }
}

impl GithubApi for GhCli {
    fn describe(&self) -> String {
        "gh CLI".into()
    }
    fn list_runs(
        &self,
        repo: &str,
        workflow: &str,
        branch: &str,
        per_page: u32,
    ) -> anyhow::Result<Vec<RunInfo>> {
        let path = format!(
            "repos/{repo}/actions/workflows/{workflow}/runs?branch={branch}&status=completed&per_page={per_page}"
        );
        let page: RunsPage = serde_json::from_slice(&self.api(&path)?)?;
        Ok(page.workflow_runs)
    }
    fn list_artifacts(&self, repo: &str, run_id: u64) -> anyhow::Result<Vec<ArtifactInfo>> {
        let page: ArtifactsPage = serde_json::from_slice(
            &self.api(&format!("repos/{repo}/actions/runs/{run_id}/artifacts?per_page=100"))?,
        )?;
        Ok(page.artifacts)
    }
    fn download_artifact(&self, repo: &str, art: &ArtifactInfo, dest: &Path) -> anyhow::Result<()> {
        let bytes = self.api(&format!("repos/{repo}/actions/artifacts/{}/zip", art.id))?;
        std::fs::write(dest, bytes)?;
        Ok(())
    }
}

pub struct TokenHttp {
    token: String,
}

impl TokenHttp {
    fn client(&self) -> anyhow::Result<reqwest::blocking::Client> {
        Ok(reqwest::blocking::Client::builder()
            .user_agent(format!("cockpit/{}", crate::VERSION))
            .timeout(std::time::Duration::from_secs(120))
            .build()?)
    }
    fn get(&self, url: &str) -> anyhow::Result<reqwest::blocking::Response> {
        with_retry(|| {
            let res = self
                .client()?
                .get(url)
                .bearer_auth(&self.token)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .send()?;
            if res.status() == 429 || res.status().as_u16() >= 500 {
                let retry_after = res
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(2)
                    .min(60);
                std::thread::sleep(std::time::Duration::from_secs(retry_after));
                anyhow::bail!("HTTP {} from GitHub", res.status());
            }
            if !res.status().is_success() {
                anyhow::bail!("HTTP {} from GitHub for {}", res.status(), redact(url));
            }
            Ok(res)
        })
    }
}

fn redact(url: &str) -> String {
    url.split('?').next().unwrap_or(url).to_string()
}

impl GithubApi for TokenHttp {
    fn describe(&self) -> String {
        "GITHUB_TOKEN".into()
    }
    fn list_runs(
        &self,
        repo: &str,
        workflow: &str,
        branch: &str,
        per_page: u32,
    ) -> anyhow::Result<Vec<RunInfo>> {
        let url = format!(
            "https://api.github.com/repos/{repo}/actions/workflows/{workflow}/runs?branch={branch}&status=completed&per_page={per_page}"
        );
        let page: RunsPage = self.get(&url)?.json()?;
        Ok(page.workflow_runs)
    }
    fn list_artifacts(&self, repo: &str, run_id: u64) -> anyhow::Result<Vec<ArtifactInfo>> {
        let url = format!(
            "https://api.github.com/repos/{repo}/actions/runs/{run_id}/artifacts?per_page=100"
        );
        let page: ArtifactsPage = self.get(&url)?.json()?;
        Ok(page.artifacts)
    }
    fn download_artifact(&self, repo: &str, art: &ArtifactInfo, dest: &Path) -> anyhow::Result<()> {
        let url = art.archive_download_url.clone().unwrap_or_else(|| {
            format!("https://api.github.com/repos/{repo}/actions/artifacts/{}/zip", art.id)
        });
        let mut res = self.get(&url)?;
        let mut f = std::fs::File::create(dest)?;
        std::io::copy(&mut res, &mut f)?;
        Ok(())
    }
}
