use super::output::Format;
use crate::cockpit::Cockpit;
use crate::model::{HealthState, Severity};
use crate::time::{ago, now};

pub fn run(
    c: &Cockpit,
    limit: usize,
    all: bool,
    min_severity: Option<&str>,
    feature: Option<&str>,
    fmt: &Format,
) -> anyhow::Result<i32> {
    let st = c.state.load();
    let min = min_severity.map(Severity::parse);
    let items: Vec<_> = st
        .attention
        .iter()
        .filter(|a| all || a.severity != Severity::Informational)
        .filter(|a| min.map(|m| a.severity <= m).unwrap_or(true))
        .filter(|a| {
            feature
                .map(|f| a.feature.as_ref().map(|x| x.as_str() == f).unwrap_or(false))
                .unwrap_or(true)
        })
        .take(limit)
        .collect();
    if fmt.json {
        fmt.json_out(
            "cockpit.attention.v1",
            &st.project,
            serde_json::json!({ "items": items, "total": st.attention.len() }),
        );
        return Ok(0);
    }
    if items.is_empty() {
        println!("{}", fmt.dim("Nothing needs attention."));
        return Ok(0);
    }
    let t_now = now();
    for a in items {
        let state = a
            .feature
            .as_ref()
            .and_then(|f| st.features.get(f))
            .map(|f| f.health.state)
            .unwrap_or(HealthState::Attention);
        let who = match (&a.feature, &a.feature_title) {
            (Some(id), Some(t)) => format!("{} · {}", id.as_str(), t),
            _ => a.subject.clone(),
        };
        println!("{}  {:<10} {}", fmt.icon(state), fmt.dim(a.severity.as_str()), fmt.bold(&who));
        println!("    {}  {}", a.title, a.summary);
        println!(
            "    {}",
            fmt.dim(&format!("{} · since {}", a.why_it_matters, ago(a.first_seen, t_now)))
        );
    }
    Ok(0)
}
