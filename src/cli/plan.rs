//! Print a compact staged verification plan without running builds or tests.
use super::output::Format;
use crate::agent::{self, FastBackend, Mode};
use crate::cockpit::Cockpit;

pub fn run(
    c: &Cockpit,
    files: &[String],
    base: Option<&str>,
    mode: Mode,
    fast_backend: FastBackend,
    fmt: &Format,
) -> anyhow::Result<i32> {
    let (changed, source) = agent::changes(&c.located.root, files, base)?;
    let plan = agent::analyze(&c.located.root, &changed, source, mode, fast_backend)?;
    if fmt.json {
        let state = c.state.load();
        fmt.json_out("cockpit.plan.v1", &state.project, &plan);
    } else {
        println!("Change scope: {} ({} changed files)", plan.scope, plan.changed_count);
        if !plan.projects.is_empty() {
            println!("Projects: {}", plan.projects.join(", "));
        }
        if !plan.unknown_inputs.is_empty() || !plan.unmapped_files.is_empty() {
            println!("Scope unknown: review unmapped/configuration inputs before selecting checks.");
        }
        if plan.actions.is_empty() {
            println!("No local commands recommended at this stage.");
        }
        for a in &plan.actions {
            println!("  [{}] (cd {}) {}", a.stage, a.cwd, a.command.join(" "));
        }
        if plan.authoritative_ci_required {
            println!("Existing CI remains required; no verification was executed.");
        }
    }
    Ok(super::EXIT_OK)
}
