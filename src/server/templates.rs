//! MiniJinja environment: embedded templates in release, disk templates in `--dev`.

use super::view;
use minijinja::{Environment, Value};
use rust_embed::RustEmbed;
use std::path::PathBuf;

#[derive(RustEmbed)]
#[folder = "ui/templates/"]
struct Embedded;

pub struct Templates {
    dev_dir: Option<PathBuf>,
    env: Environment<'static>,
}

impl Templates {
    pub fn new(ui_dir: Option<PathBuf>) -> anyhow::Result<Templates> {
        let dev_dir = ui_dir.map(|d| d.join("templates"));
        let env = build_env(dev_dir.as_deref())?;
        Ok(Templates { dev_dir, env })
    }

    pub fn render(&self, name: &str, ctx: Value) -> Result<String, minijinja::Error> {
        if let Some(dir) = &self.dev_dir {
            // Dev mode: rebuild the environment per render so template edits show up live.
            let env = build_env(Some(dir)).map_err(|e| {
                minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
            })?;
            return env.get_template(name)?.render(ctx);
        }
        self.env.get_template(name)?.render(ctx)
    }

    /// Parse every embedded template so syntax errors fail at startup rather than per page.
    pub fn check_all(&self) -> anyhow::Result<()> {
        for name in Embedded::iter() {
            self.env.get_template(&name).map_err(|e| anyhow::anyhow!("template {name}: {e}"))?;
        }
        Ok(())
    }
}

fn build_env(dev_dir: Option<&std::path::Path>) -> anyhow::Result<Environment<'static>> {
    let mut env = Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
    match dev_dir {
        Some(dir) => env.set_loader(minijinja::path_loader(dir)),
        None => env.set_loader(|name| match Embedded::get(name) {
            Some(f) => Ok(Some(String::from_utf8_lossy(&f.data).to_string())),
            None => Ok(None),
        }),
    }
    view::register(&mut env);
    Ok(env)
}
