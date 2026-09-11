//! Per-test history: last N runs, flakiness, duration trend.

use crate::config::HealthSection;
use crate::model::*;

pub struct History {
    pub flaky: bool,
    pub flaky_text: Option<String>,
    /// (previous median ms, latest ms, relative change)
    pub duration_trend: Option<(u64, u64, f64)>,
}

/// `runs` newest first.
pub fn analyse(runs: &[TestRun], cfg: &HealthSection) -> History {
    let window: Vec<&TestRun> = runs.iter().take(cfg.flaky_window as usize).collect();
    let latest = window.first();
    let failures = window
        .iter()
        .filter(|r| matches!(r.status, TestStatus::Failed | TestStatus::Flaky))
        .count() as u32;
    let latest_flaky = latest.map(|r| r.status == TestStatus::Flaky).unwrap_or(false);
    let consistently_failing = latest.map(|r| r.status == TestStatus::Failed).unwrap_or(false);
    let flaky = latest_flaky
        || (failures >= cfg.flaky_failures
            && !consistently_failing
            && window.len() >= cfg.flaky_failures as usize);
    let flaky_text = if flaky {
        Some(if latest_flaky {
            "passed only after a retry in the latest run".to_string()
        } else {
            format!(
                "{failures} failures in the last {} runs (threshold {})",
                window.len(),
                cfg.flaky_failures
            )
        })
    } else {
        None
    };
    let mut duration_trend = None;
    if let Some(l) = latest.and_then(|r| r.duration_ms) {
        let mut prev: Vec<u64> =
            window.iter().skip(1).take(10).filter_map(|r| r.duration_ms).collect();
        if prev.len() >= 3 {
            prev.sort();
            let median = prev[prev.len() / 2];
            if median > 0 {
                let rel = (l as f64 - median as f64) / median as f64;
                if rel * 100.0 >= cfg.duration_regression_pct
                    && l.saturating_sub(median) >= cfg.duration_regression_min_ms
                {
                    duration_trend = Some((median, l, rel));
                }
            }
        }
    }
    History { flaky, flaky_text, duration_trend }
}
