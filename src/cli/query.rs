//! `cockpit query '<expr>' [--format table|ids|count|json]`

use super::output::Format;
use super::{EXIT_OK, EXIT_USAGE};
use crate::cockpit::Cockpit;
use crate::query::{Rows, parse, run as run_query};

pub fn run(c: &Cockpit, expr: &str, format: &str, fmt: &Format) -> anyhow::Result<i32> {
    let q = match parse(expr) {
        Ok(q) => q,
        Err(e) => {
            fmt.error(EXIT_USAGE, "query", &format!("cannot parse query: {e}"));
            return Ok(EXIT_USAGE);
        }
    };
    let st = c.state.load();
    let rows = match run_query(&st, &q) {
        Ok(r) => r,
        Err(e) => {
            fmt.error(EXIT_USAGE, "query", &e);
            return Ok(EXIT_USAGE);
        }
    };
    let (ids, table, json): (Vec<String>, Vec<Vec<String>>, Vec<serde_json::Value>) = match &rows {
        Rows::Features(v) => (
            v.iter().map(|f| f.feature.id.to_string()).collect(),
            v.iter()
                .map(|f| {
                    vec![
                        fmt.glyph(f.health.state),
                        f.feature.id.to_string(),
                        f.feature.title.chars().take(48).collect(),
                        f.feature.importance.as_str().into(),
                        f.health.headline.chars().take(60).collect(),
                    ]
                })
                .collect(),
            v.iter().map(|f| crate::server::routes::feature_detail_json(&st, f)).collect(),
        ),
        Rows::Tests(v) => (
            v.iter().map(|t| t.test.id.to_string()).collect(),
            v.iter()
                .map(|t| {
                    vec![
                        t.latest.as_ref().map(|r| r.status.as_str().to_string()).unwrap_or_else(|| "?".into()),
                        t.test.id.to_string(),
                        t.test.name.chars().take(70).collect(),
                        t.test.kind.as_str().into(),
                        t.features.iter().map(|(f, _)| f.to_string()).collect::<Vec<_>>().join(","),
                    ]
                })
                .collect(),
            v.iter().map(|t| serde_json::to_value(crate::server::routes::test_row(&st, t)).unwrap_or_default()).collect(),
        ),
        Rows::Benchmarks(v) => (
            v.iter().map(|b| b.benchmark.id.to_string()).collect(),
            v.iter()
                .map(|b| {
                    vec![
                        b.comparison.as_ref().map(|c| c.classification.as_str().to_string()).unwrap_or_else(|| "unknown".into()),
                        b.benchmark.id.to_string(),
                        b.label().chars().take(60).collect(),
                        b.latest.as_ref().map(|r| crate::state::benchmark_cmp::fmt_value(r.measurement.value, &b.benchmark.metric.unit)).unwrap_or_default(),
                        b.comparison.as_ref().and_then(|c| c.relative_change).map(|r| format!("{:+.1}%", r * 100.0)).unwrap_or_default(),
                    ]
                })
                .collect(),
            v.iter().map(|b| serde_json::to_value(crate::server::perf::benchmark_row(&st, b)).unwrap_or_default()).collect(),
        ),
        Rows::Adrs(v) => (
            v.iter().map(|a| a.adr.id.to_string()).collect(),
            v.iter().map(|a| vec![a.adr.status.as_str().into(), a.adr.id.to_string(), a.adr.title.chars().take(70).collect(), a.adr.date.map(|d| d.to_string()).unwrap_or_default(), a.features.len().to_string()]).collect(),
            v.iter().map(|a| serde_json::json!({ "id": a.adr.id, "title": a.adr.title, "status": a.adr.status.as_str(), "date": a.adr.date.map(|d| d.to_string()), "features": a.features })).collect(),
        ),
    };
    match format {
        "count" => println!("{}", ids.len()),
        "ids" => {
            for id in &ids {
                println!("{id}");
            }
        }
        "json" => {
            fmt.json_out("cockpit.query.v1", &st.project, serde_json::json!({ "query": expr, "entity": q.entity, "count": ids.len(), "items": json }));
        }
        _ => {
            if fmt.json {
                fmt.json_out("cockpit.query.v1", &st.project, serde_json::json!({ "query": expr, "entity": q.entity, "count": ids.len(), "items": json }));
            } else {
                if table.is_empty() {
                    println!("{}", fmt.dim("no matches"));
                }
                let widths: Vec<usize> = (0..5)
                    .map(|i| {
                        table
                            .iter()
                            .map(|r| r.get(i).map(|s| s.chars().count()).unwrap_or(0))
                            .max()
                            .unwrap_or(0)
                    })
                    .collect();
                for r in &table {
                    let cells: Vec<String> = r
                        .iter()
                        .enumerate()
                        .map(|(i, s)| format!("{:<w$}", s, w = widths[i]))
                        .collect();
                    println!("{}", cells.join("  ").trim_end());
                }
                println!("{}", fmt.dim(&format!("{} {}", ids.len(), q.entity)));
            }
        }
    }
    Ok(EXIT_OK)
}
