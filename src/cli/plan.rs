//! Print a compact staged verification plan without running builds or tests.
use super::output::Format;
use crate::agent::{self, FastBackend, Mode};
use crate::cockpit::Cockpit;
use crate::config::{Config, DevelopmentSection, Downstream};
use std::path::Path;

pub struct Options<'a> {
    pub files: &'a [String],
    pub base: Option<&'a str>,
    pub mode: Mode,
    pub fast_backend: FastBackend,
    pub include_evidence: bool,
    pub select_tests: bool,
    pub downstream: Option<Downstream>,
    pub policy: Option<&'a Path>,
}

/// The `[development]` policy for this run: the project's, or that of `--policy FILE` (a
/// `cockpit.toml` kept outside the repository, e.g. an umbrella registry's), with an explicit
/// `--downstream` on top.
pub fn policy(
    c: &Cockpit,
    file: Option<&Path>,
    downstream: Option<Downstream>,
) -> anyhow::Result<DevelopmentSection> {
    let mut policy = match file {
        Some(path) => Config::load(path)?.development,
        None => c.located.config.development.clone(),
    };
    if let Some(downstream) = downstream {
        policy.downstream = downstream;
    }
    Ok(policy)
}

pub fn run(c: &Cockpit, options: Options<'_>, fmt: &Format) -> anyhow::Result<i32> {
    let Options {
        files,
        base,
        mode,
        fast_backend,
        include_evidence,
        select_tests,
        downstream,
        policy: policy_file,
    } = options;
    let policy = policy(c, policy_file, downstream)?;
    let (changed, source) = agent::changes(&c.located.root, files, base)?;
    let mut plan =
        agent::analyze_with_policy(&c.located.root, &changed, source, mode, fast_backend, &policy)?;
    if include_evidence {
        agent::attach_prior_evidence(&mut plan, &c.state.load(), &changed);
    }
    if select_tests {
        let tests = crate::agent_tests::select(&c.located.root, &c.state.load(), &changed);
        if mode == Mode::Edit && plan.scope != "unknown" {
            plan.actions.extend(tests.actions);
        }
        plan.test_selection = Some(tests.selection);
    }
    if fmt.json {
        let state = c.state.load();
        fmt.json_out("cockpit.plan.v1", &state.project, &plan);
    } else {
        println!("Change scope: {} ({} changed files)", plan.scope, plan.changed_count);
        if !plan.projects.is_empty() {
            println!("Projects: {}", plan.projects.join(", "));
        }
        if !plan.downstream_projects.is_empty() {
            println!("Downstream ({}): {}", plan.downstream, plan.downstream_projects.join(", "));
        }
        let classes = &plan.path_classes;
        if classes.test_data + classes.generated + classes.documentation > 0
            || !classes.workspace_inputs.is_empty()
        {
            println!(
                "Classified: {} test data, {} generated, {} documentation, {} workspace-wide inputs",
                classes.test_data,
                classes.generated,
                classes.documentation,
                classes.workspace_inputs.len()
            );
        }
        if !plan.unknown_inputs.is_empty() || !plan.unmapped_files.is_empty() {
            println!(
                "Scope unknown: review unmapped/configuration inputs before selecting checks."
            );
            for input in plan.unknown_inputs.iter().chain(&plan.unmapped_files).take(8) {
                println!("  ? {input}");
            }
            for note in &plan.notes {
                println!("  - {note}");
            }
        }
        if plan.actions.is_empty() {
            println!("No local commands recommended at this stage.");
        }
        for a in &plan.actions {
            println!("  [{}] (cd {}) {}", a.stage, a.cwd, a.command.join(" "));
        }
        if let Some(prior) = &plan.prior_evidence {
            println!(
                "Prior scan: {} · {} matching features, {} linked test hints (historical only)",
                prior.state, prior.matched_feature_count, prior.linked_test_count
            );
        }
        if let Some(selection) = &plan.test_selection {
            println!(
                "Test selection: {} ({} linked test candidates, {} runnable Playwright specs)",
                selection.status, selection.total_candidates, selection.runnable_actions
            );
        }
        if plan.authoritative_ci_required {
            println!("Existing CI remains required; no verification was executed.");
        }
    }
    Ok(super::EXIT_OK)
}
