use super::output::Format;
use crate::cockpit::Cockpit;

pub struct Options {
    pub watch: bool,
    pub port: Option<u16>,
    pub bind: Option<String>,
    pub open: bool,
    pub dev: bool,
    pub no_scan: bool,
}

pub fn run(c: &mut Cockpit, opts: Options, fmt: &Format) -> anyhow::Result<i32> {
    if !opts.no_scan {
        c.scan(None)?;
    }
    let bind = opts.bind.clone().unwrap_or_else(|| c.located.config.server.bind.clone());
    let port = opts.port.unwrap_or(c.located.config.server.port);
    if bind != "127.0.0.1" && bind != "localhost" && bind != "::1" {
        eprintln!(
            "{} binding to a non-loopback address ({bind}); anyone on the network can read this project's state",
            fmt.paint("33", "warning:")
        );
    }
    let st = c.state.load();
    let repos = st.project.repos.iter().filter(|r| r.head.is_some()).count();
    println!(
        "cockpit {} · {} · http://{}:{} · {} repo{}{}",
        crate::VERSION,
        st.project.name,
        bind,
        port,
        repos,
        if repos == 1 { "" } else { "s" },
        if opts.watch { " · watching" } else { "" }
    );
    crate::server::serve_blocking(
        c,
        crate::server::ServeOptions {
            bind,
            port,
            open: opts.open,
            dev: opts.dev,
            watch: opts.watch,
        },
    )?;
    Ok(0)
}
