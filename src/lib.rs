//! Project Cockpit — a local-first engineering instrument panel.
//!
//! The crate is organised as a pipeline: `config` locates and describes a project, `scan`
//! walks it and hands files to `import` adapters that produce normalised `model` entities,
//! `git` supplies temporal context, `link` connects evidence to features, `state` derives
//! health / attention / freshness, `store` persists the derived index (a deletable cache with
//! a history of runs), and `server` renders it. `cli` is the human and agent entry point.

pub mod agent;
pub mod cli;
pub mod cockpit;
pub mod config;
pub mod git;
pub mod hash;
pub mod ids;
pub mod import;
pub mod link;
pub mod migrate;
pub mod model;
pub mod paths;
pub mod pull;
pub mod query;
pub mod scan;
pub mod server;
pub mod state;
pub mod store;
pub mod time;
pub mod usage;
pub mod validate;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
