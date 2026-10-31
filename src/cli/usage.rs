//! Read-only metadata aggregation for Claude Code CLI sessions.
use super::output::Format;
use crate::cockpit::Cockpit;
use crate::usage;
use std::path::Path;

pub fn run(c: &Cockpit, transcripts: Option<&Path>, fmt: &Format) -> anyhow::Result<i32> {
    let dir = match transcripts {
        Some(path) => path.to_path_buf(),
        None => usage::default_transcripts_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot find home directory; pass --transcripts-dir"))?,
    };
    let summary = usage::summarize(&c.located.root, &dir)?;
    if fmt.json {
        let state = c.state.load();
        fmt.json_out("cockpit.usage.v1", &state.project, &summary);
    } else {
        println!(
            "Claude Code: {} requests in {} sessions ({} transcripts inspected)",
            summary.requests, summary.matched_sessions, summary.transcript_files_scanned
        );
        println!(
            "Tokens: {} input, {} output, {} cache read, {} cache creation",
            summary.tokens.input,
            summary.tokens.output,
            summary.tokens.cache_read,
            summary.tokens.cache_creation,
        );
        for model in &summary.models {
            println!(
                "  {}: {} requests, {} observed tokens",
                model.model,
                model.requests,
                model.tokens.input
                    + model.tokens.output
                    + model.tokens.cache_read
                    + model.tokens.cache_creation
            );
        }
        println!("Cost not estimated. Session spans are not active work time.");
    }
    Ok(super::EXIT_OK)
}
