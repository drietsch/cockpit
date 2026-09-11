//! `cockpit pull`: fetch CI artifacts, then rescan.

use super::output::Format;
use super::{EXIT_OK, EXIT_POLICY};
use crate::cockpit::Cockpit;
use crate::pull::{Options, Outcome};

pub fn run(c: &mut Cockpit, opts: Options, strict: bool, fmt: &Format) -> anyhow::Result<i32> {
    let dry = opts.dry_run;
    let reports = crate::pull::run(&c.located, &opts, fmt)?;
    let pulled = reports.iter().filter(|r| matches!(r.outcome, Outcome::Pulled { .. })).count();
    let failed = reports.iter().filter(|r| matches!(r.outcome, Outcome::Failed(_))).count();
    let skipped = reports.iter().filter(|r| matches!(r.outcome, Outcome::Skipped(_))).count();
    if fmt.json {
        let items: Vec<serde_json::Value> = reports
            .iter()
            .map(|r| {
                let (status, detail) = match &r.outcome {
                    Outcome::Pulled { run_id, artifact_id, files, bytes } => ("pulled", serde_json::json!({ "run_id": run_id, "artifact_id": artifact_id, "files": files, "bytes": bytes })),
                    Outcome::UpToDate { run_id } => ("up_to_date", serde_json::json!({ "run_id": run_id })),
                    Outcome::Skipped(why) => ("skipped", serde_json::json!({ "reason": why })),
                    Outcome::Failed(why) => ("failed", serde_json::json!({ "error": why })),
                };
                serde_json::json!({ "source": r.source, "github": r.github, "status": status, "detail": detail, "commit": r.commit, "finished_at": r.finished_at, "url": r.url })
            })
            .collect();
        let st = c.state.load();
        fmt.json_out(
            "cockpit.pull.v1",
            &st.project,
            serde_json::json!({ "dry_run": dry, "sources": items, "pulled": pulled, "failed": failed, "skipped": skipped }),
        );
    } else if reports.is_empty() {
        println!("{}", fmt.dim("No evidence source has [evidence.github]; nothing to pull."));
    } else {
        println!();
        println!(
            "{} pulled · {} up to date · {} skipped · {} failed{}",
            pulled,
            reports.iter().filter(|r| matches!(r.outcome, Outcome::UpToDate { .. })).count(),
            skipped,
            failed,
            if dry { "  (dry run)" } else { "" }
        );
    }
    if pulled > 0 && !dry {
        c.scan(None)?;
        if !fmt.json {
            println!("{}", fmt.dim("rescanned"));
        }
    }
    Ok(if failed > 0 || (strict && skipped > 0) { EXIT_POLICY } else { EXIT_OK })
}
