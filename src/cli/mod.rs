//! Command-line entry point. Exit codes: 0 ok · 1 policy/pull failure · 2 usage/config error ·
//! 3 project not found · 4 index error.

pub mod attention;
pub mod doctor;
pub mod export;
pub mod feature;
pub mod init;
pub mod output;
pub mod plan;
pub mod pull;
pub mod query;
pub mod scan;
pub mod serve;
pub mod status;
pub mod usage;
pub mod verify;

use crate::cockpit::{Cockpit, CockpitError};
use crate::config::ConfigError;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub const EXIT_OK: i32 = 0;
pub const EXIT_POLICY: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_NO_PROJECT: i32 = 3;
pub const EXIT_INDEX: i32 = 4;

#[derive(Parser, Debug)]
#[command(name = "cockpit", version, about = "Local-first engineering cockpit for a repository")]
pub struct Cli {
    /// Project root or cockpit.toml (default: discovered from the current directory)
    #[arg(short = 'C', long, global = true, env = "COCKPIT_PROJECT")]
    pub project: Option<PathBuf>,
    /// Machine-readable JSON on stdout
    #[arg(long, global = true)]
    pub json: bool,
    /// Log level: error, warn, info, debug, trace
    #[arg(long, global = true, env = "COCKPIT_LOG", default_value = "warn")]
    pub log: String,
    /// Disable colour
    #[arg(long, global = true, env = "NO_COLOR")]
    pub no_color: bool,
    /// Replace status glyphs with ASCII words
    #[arg(long, global = true, env = "COCKPIT_ASCII")]
    pub ascii: bool,
    /// Override the index path (default: .cockpit/index.sqlite beside the config)
    #[arg(long, global = true)]
    pub index: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Scan the project and serve the web UI
    Serve {
        #[arg(long)]
        watch: bool,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        bind: Option<String>,
        #[arg(long)]
        open: bool,
        /// Read templates and assets from ./ui on disk
        #[arg(long)]
        dev: bool,
        #[arg(long)]
        no_scan: bool,
        /// Run `cockpit pull` periodically (e.g. 15m, 1h)
        #[arg(long)]
        pull_every: Option<String>,
    },
    /// Import project state into the index
    Scan {
        /// Re-import every file (history is kept)
        #[arg(long)]
        full: bool,
        /// With --full: also drop run history
        #[arg(long)]
        reset_history: bool,
    },
    /// Terminal summary of the project state
    Status {
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
    /// Check policies (references, required evidence, freshness, drift)
    Validate {
        #[arg(long)]
        strict: bool,
    },
    /// Health, reasons and evidence for one feature
    Feature { id: String },
    /// Write every feature with its claims and health as one JSON document (cockpit.export.v1)
    Export {
        /// public (no notes on evidence, commits or attention) or full
        #[arg(long, default_value = "public")]
        profile: String,
        /// Write to this file instead of stdout
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Ranked attention items
    Attention {
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Include informational items (unverified claims, unlinked tests)
        #[arg(long)]
        all: bool,
        #[arg(long)]
        min_severity: Option<String>,
        #[arg(long)]
        feature: Option<String>,
    },
    /// Environment and project diagnostics
    Doctor,
    /// Scaffold cockpit.toml and docs/features
    Init {
        #[arg(long)]
        from_cargo: bool,
    },
    /// Convert another state system into Cockpit conventions
    Migrate {
        #[command(subcommand)]
        what: MigrateCommand,
    },
    /// Fetch the latest CI artifacts for configured evidence sources
    Pull {
        /// Decide what would be fetched without downloading
        #[arg(long)]
        dry_run: bool,
        /// Re-download even when the sidecar already records the newest run
        #[arg(long)]
        force: bool,
        /// Only sources whose id matches (glob; repeatable)
        #[arg(long)]
        source: Vec<String>,
        /// Exit 1 when any source was skipped
        #[arg(long)]
        strict: bool,
    },
    /// Suggest minimal, staged verification for changed files; execute nothing
    Plan {
        /// Explicit project-root-relative changed file (repeatable); defaults to Git changes
        #[arg(long = "file")]
        files: Vec<String>,
        /// Compare the worktree against this git revision (default: HEAD)
        #[arg(long)]
        base: Option<String>,
        /// edit | checkpoint | ci
        #[arg(long, value_enum, default_value = "edit")]
        mode: crate::agent::Mode,
        /// none | blaze; Blaze is advisory and never a CI substitute
        #[arg(long, value_enum, default_value = "auto")]
        fast_backend: crate::agent::FastBackend,
        /// Add bounded feature/test hints from the last Cockpit scan (historical, not verification)
        #[arg(long)]
        include_evidence: bool,
    },
    /// Aggregate local Claude Code CLI token usage for this repository; metadata only
    Usage {
        /// Claude transcript root (default ~/.claude/projects)
        #[arg(long)]
        transcripts_dir: Option<PathBuf>,
    },
    /// Execute selected verification commands (explicit; logs stay local)
    Verify {
        /// Explicit project-root-relative changed file (repeatable)
        #[arg(long = "file")]
        files: Vec<String>,
        /// Compare the worktree against this revision
        #[arg(long)]
        base: Option<String>,
        /// checkpoint or ci
        #[arg(long, value_enum, default_value = "checkpoint")]
        mode: crate::agent::Mode,
        /// Override the shared Rust edit backend
        #[arg(long, value_enum, default_value = "auto")]
        fast_backend: crate::agent::FastBackend,
    },
    /// Structured query over project state
    Query {
        expr: String,
        /// table | ids | count | json
        #[arg(long, default_value = "table")]
        format: String,
    },
    /// Shell completions (bash, zsh, fish, powershell, elvish)
    Completions { shell: clap_complete::Shell },
}

#[derive(Subcommand, Debug)]
pub enum MigrateCommand {
    /// Migrate a paged-media/state registry into feature Markdown + config
    PagedState {
        state_dir: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        config_out: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        force: bool,
    },
}

pub fn run() -> i32 {
    let cli = Cli::parse();
    init_logging(&cli.log);
    let fmt = output::Format {
        json: cli.json,
        color: !cli.no_color && std::io::IsTerminal::is_terminal(&std::io::stdout()),
        ascii: cli.ascii,
    };
    match dispatch(&cli, &fmt) {
        Ok(code) => code,
        Err(e) => {
            let (code, kind) = classify(&e);
            fmt.error(code, kind, &e.to_string());
            code
        }
    }
}

fn dispatch(cli: &Cli, fmt: &output::Format) -> anyhow::Result<i32> {
    let cwd = std::env::current_dir()?;
    match &cli.command {
        Command::Scan { full, reset_history } => {
            let mut c = Cockpit::open(&cwd, cli.project.as_deref(), cli.index.as_deref())?;
            scan::run(&mut c, *full, *reset_history, fmt)
        }
        Command::Status { limit } => {
            let c = open_scanned(cli)?;
            status::run(&c, *limit, fmt)
        }
        Command::Doctor => {
            let c = Cockpit::open(&cwd, cli.project.as_deref(), cli.index.as_deref())?;
            doctor::run(&c, fmt)
        }
        Command::Serve { watch, port, bind, open, dev, no_scan, pull_every } => {
            let mut c = Cockpit::open(&cwd, cli.project.as_deref(), cli.index.as_deref())?;
            serve::run(
                &mut c,
                serve::Options {
                    watch: *watch,
                    port: *port,
                    bind: bind.clone(),
                    open: *open,
                    dev: *dev,
                    no_scan: *no_scan,
                    pull_every: pull_every.clone(),
                },
                fmt,
            )
        }
        Command::Migrate { what } => match what {
            MigrateCommand::PagedState { state_dir, out, config_out, dry_run, force } => {
                let located = crate::config::Config::discover(&cwd, cli.project.as_deref()).ok();
                crate::migrate::paged_state::run(
                    state_dir,
                    out.as_deref(),
                    config_out.as_deref(),
                    located.as_ref(),
                    *dry_run,
                    *force,
                    fmt,
                )
            }
        },
        Command::Feature { id } => {
            let c = open_scanned(cli)?;
            feature::run(&c, id, fmt)
        }
        Command::Export { profile, out } => {
            let c = open_scanned(cli)?;
            export::run(&c, profile, out.as_deref(), fmt)
        }
        Command::Attention { limit, all, min_severity, feature } => {
            let c = open_scanned(cli)?;
            attention::run(&c, *limit, *all, min_severity.as_deref(), feature.as_deref(), fmt)
        }
        Command::Validate { strict } => {
            let c = open_scanned(cli)?;
            crate::validate::run(&c, *strict, fmt)
        }
        Command::Pull { dry_run, force, source, strict } => {
            let mut c = Cockpit::open(&cwd, cli.project.as_deref(), cli.index.as_deref())?;
            pull::run(
                &mut c,
                crate::pull::Options { dry_run: *dry_run, force: *force, only: source.clone() },
                *strict,
                fmt,
            )
        }
        Command::Init { from_cargo } => {
            let root = match cli.project.as_deref() {
                Some(p) if p.is_file() => {
                    p.parent().map(|d| d.to_path_buf()).unwrap_or(cwd.clone())
                }
                Some(p) => p.to_path_buf(),
                None => cwd.clone(),
            };
            init::run(&root, *from_cargo, fmt)
        }
        Command::Plan { files, base, mode, fast_backend, include_evidence } => {
            let c = Cockpit::open(&cwd, cli.project.as_deref(), cli.index.as_deref())?;
            plan::run(&c, files, base.as_deref(), *mode, *fast_backend, *include_evidence, fmt)
        }
        Command::Verify { files, base, mode, fast_backend } => {
            let c = Cockpit::open(&cwd, cli.project.as_deref(), cli.index.as_deref())?;
            verify::run(&c, files, base.as_deref(), *mode, *fast_backend, fmt)
        }
        Command::Usage { transcripts_dir } => {
            let c = Cockpit::open(&cwd, cli.project.as_deref(), cli.index.as_deref())?;
            usage::run(&c, transcripts_dir.as_deref(), fmt)
        }
        Command::Query { expr, format } => {
            let c = open_scanned(cli)?;
            query::run(&c, expr, format, fmt)
        }
        Command::Completions { shell } => {
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            clap_complete::generate(*shell, &mut cmd, "cockpit", &mut std::io::stdout());
            Ok(EXIT_OK)
        }
    }
}

/// Open the project and bring the index up to date (an incremental scan is cheap; a stale
/// answer from a gate is not).
fn open_scanned(cli: &Cli) -> anyhow::Result<Cockpit> {
    let cwd = std::env::current_dir()?;
    let mut c = Cockpit::open(&cwd, cli.project.as_deref(), cli.index.as_deref())?;
    c.scan(None)?;
    Ok(c)
}

fn classify(e: &anyhow::Error) -> (i32, &'static str) {
    if let Some(ce) = e.downcast_ref::<CockpitError>() {
        return match ce {
            CockpitError::Config(ConfigError::NotFound(_)) => (EXIT_NO_PROJECT, "no_project"),
            CockpitError::Config(_) => (EXIT_USAGE, "config"),
            CockpitError::Store(_) => (EXIT_INDEX, "index"),
            CockpitError::Scan(_) => (EXIT_INDEX, "scan"),
        };
    }
    if let Some(ce) = e.downcast_ref::<ConfigError>() {
        return match ce {
            ConfigError::NotFound(_) => (EXIT_NO_PROJECT, "no_project"),
            _ => (EXIT_USAGE, "config"),
        };
    }
    (EXIT_USAGE, "error")
}

fn init_logging(level: &str) {
    use tracing_subscriber::EnvFilter;
    let filter = std::env::var("RUST_LOG")
        .ok()
        .unwrap_or_else(|| format!("cockpit={level},project_cockpit={level},warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new("warn")))
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();
}
