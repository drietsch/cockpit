//! Execute an explicitly requested verification plan, with compact output.
//! A local success is not automatically a Cockpit feature-health verdict.
use super::output::Format;
use crate::agent::{self, FastBackend, Mode};
use crate::cockpit::Cockpit;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
struct Step {
    stage: &'static str,
    command: Vec<String>,
    cwd: String,
    authority: &'static str,
    status: &'static str,
    exit_code: Option<i32>,
    duration_ms: u128,
    local_log: String,
    output_tail: String,
}

#[derive(Debug, Serialize)]
struct Report {
    plan_source: String,
    scope: &'static str,
    verification_status: &'static str,
    authoritative_ci_still_required: bool,
    steps: Vec<Step>,
    notes: Vec<&'static str>,
}

// Only the explicitly optional CI preflight is nonblocking. Edit-time
// checks and selected tests are actionable failures when explicitly run.
fn is_nonblocking_preflight(authority: &str) -> bool {
    authority == "advisory"
}

fn verification_status(hard_failure: bool, advisory_failure: bool) -> &'static str {
    if hard_failure {
        "failed"
    } else if advisory_failure {
        "passed_selected_checks_with_advisory_failures"
    } else {
        "passed_selected_checks"
    }
}

fn output_tail(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(4096)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(text.chars().rev().take(1500).collect::<String>().chars().rev().collect())
}

pub fn run(
    c: &Cockpit,
    files: &[String],
    base: Option<&str>,
    mode: Mode,
    fast_backend: FastBackend,
    select_tests: bool,
    fmt: &Format,
) -> Result<i32> {
    let (changed, source) = agent::changes(&c.located.root, files, base)?;
    let mut plan = agent::analyze_with_policy(
        &c.located.root,
        &changed,
        source,
        mode,
        fast_backend,
        &c.located.config.development,
    )?;
    if select_tests {
        if mode != Mode::Edit {
            bail!(
                "--select-tests is for fast edit-time checks; use checkpoint/CI for broader verification"
            );
        }
        let selection = crate::agent_tests::select(&c.located.root, &c.state.load(), &changed);
        if selection.selection.status != "advisory_runnable_selection" {
            bail!(
                "no safe exact Playwright test-file selection available: {}",
                selection.selection.status
            );
        }
        plan.actions.extend(selection.actions);
    }
    if plan.scope == "unknown" {
        bail!("cannot execute a potentially incomplete verification plan: unknown change impact");
    }
    if plan.actions.is_empty() {
        bail!("verification plan contains no runnable checks; existing CI remains required");
    }
    if plan.actions.len() > 128 {
        bail!("verification plan exceeds the 128-command execution limit");
    }
    let root = c.located.root.canonicalize().context("resolving repository root")?;
    let log_dir = c.located.state_dir.join("verification");
    fs::create_dir_all(&log_dir).context("creating local verification log directory")?;
    let time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let run_id = format!("{}-{time}", std::process::id());
    let mut report = Report {
        plan_source: plan.source,
        scope: plan.scope,
        verification_status: "not_run",
        authoritative_ci_still_required: mode != Mode::Ci,
        steps: vec![],
        notes: vec![
            "Commands are chosen by the checked-in policy; execution is explicit, never triggered by plan.",
            "Build/test output is stored locally under .cockpit/verification and capped in JSON.",
            "A successful local checkpoint never replaces authoritative repository CI.",
            "CI execution covers only selected known projects; external workflows and special gates remain authoritative.",
        ],
    };
    let mut hard_failure = false;
    let mut advisory_failure = false;
    for (i, action) in plan.actions.iter().enumerate() {
        let dir = root
            .join(&action.cwd)
            .canonicalize()
            .context("resolving verification working directory")?;
        if !dir.starts_with(&root) {
            bail!("verification working directory escapes project root");
        }
        let Some(program) = action.command.first() else {
            bail!("verification action has no program");
        };
        let filename = format!("{run_id}-{i}.log");
        let filepath = log_dir.join(&filename);
        let log = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&filepath)
            .context("opening local verification log")?;
        let start = Instant::now();
        let result = Command::new(program)
            .args(&action.command[1..])
            .current_dir(&dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .status();
        let elapsed = start.elapsed().as_millis();
        let (status, exit_code) = match result {
            Ok(s) if s.success() => ("passed", s.code()),
            Ok(s) => ("failed", s.code()),
            Err(e) => {
                // The error is recorded without treating the missing executable
                // or permission failure as a successful run.
                let result = Step {
                    stage: action.stage,
                    command: action.command.clone(),
                    cwd: action.cwd.clone(),
                    authority: action.authority,
                    status: "launch_error",
                    exit_code: None,
                    duration_ms: elapsed,
                    local_log: filename,
                    output_tail: e.to_string(),
                };
                report.steps.push(result);
                if is_nonblocking_preflight(action.authority) {
                    advisory_failure = true;
                } else {
                    hard_failure = true;
                }
                if hard_failure {
                    break;
                }
                continue;
            }
        };
        report.steps.push(Step {
            stage: action.stage,
            command: action.command.clone(),
            cwd: action.cwd.clone(),
            authority: action.authority,
            status,
            exit_code,
            duration_ms: elapsed,
            local_log: filename,
            output_tail: if status == "failed" { output_tail(&filepath)? } else { String::new() },
        });
        if status == "failed" {
            if is_nonblocking_preflight(action.authority) {
                advisory_failure = true;
            } else {
                hard_failure = true;
                break;
            }
        }
    }
    report.verification_status = verification_status(hard_failure, advisory_failure);
    if fmt.json {
        fmt.json_out("cockpit.verify.v1", &c.state.load().project, &report);
    } else {
        for step in &report.steps {
            println!(
                "{} [{}] {} ({} ms)",
                step.status,
                step.authority,
                step.command.join(" "),
                step.duration_ms
            );
            if !step.output_tail.is_empty() {
                println!("{}", step.output_tail);
            }
        }
        println!("Result: {} (not a feature-health verdict)", report.verification_status);
    }
    Ok(if hard_failure { super::EXIT_POLICY } else { super::EXIT_OK })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_preflight_is_nonblocking() {
        assert!(is_nonblocking_preflight("advisory"));
        assert!(!is_nonblocking_preflight("advisory_not_full_verification"));
        assert!(!is_nonblocking_preflight("advisory_selected_test_not_complete_coverage"));
        assert!(!is_nonblocking_preflight("ci_test"));
    }

    #[test]
    fn failed_preflight_cannot_claim_clean_verification() {
        assert_eq!(verification_status(false, false), "passed_selected_checks");
        assert_eq!(
            verification_status(false, true),
            "passed_selected_checks_with_advisory_failures"
        );
        assert_eq!(verification_status(true, true), "failed");
    }
}
