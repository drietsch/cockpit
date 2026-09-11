//! `cockpit migrate paged-state` (implemented in M0 step 2).
use crate::cli::output::Format;
use crate::config::Located;
use std::path::Path;

pub fn run(
    _state_dir: &Path,
    _out: Option<&Path>,
    _config_out: Option<&Path>,
    _located: Option<&Located>,
    _dry_run: bool,
    _force: bool,
    fmt: &Format,
) -> anyhow::Result<i32> {
    fmt.error(2, "not_implemented", "migrate paged-state is not implemented yet");
    Ok(2)
}
