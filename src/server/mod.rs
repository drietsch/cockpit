//! Web server (minimal in M0: JSON status + a plain overview).
use crate::cockpit::Cockpit;

pub struct ServeOptions {
    pub bind: String,
    pub port: u16,
    pub open: bool,
    pub dev: bool,
    pub watch: bool,
}

pub fn serve_blocking(_c: &mut Cockpit, _opts: ServeOptions) -> anyhow::Result<()> {
    anyhow::bail!("the web server arrives with the next step of M0")
}
