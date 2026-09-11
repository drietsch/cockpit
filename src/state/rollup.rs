//! Per-category evidence rollups for one feature, from the latest run of each linked test and
//! the latest evidence of each linked visual check.

use super::TestView;
use super::freshness::{FreshnessInput, evaluate as freshness};
use crate::model::*;
use jiff::Timestamp;

pub struct RollupInput<'a> {
    pub tests: Vec<&'a TestView>,
    pub visuals: Vec<&'a VisualEvidence>,
    pub required: &'a [EvidenceCategory],
    pub freshness: &'a FreshnessInput<'a>,
}

pub fn rollups(input: &RollupInput) -> Vec<KindRollup> {
    let mut out = Vec::new();
    for cat in EvidenceCategory::ALL {
        if cat == EvidenceCategory::Perf {
            continue; // benchmarks roll up in state/benchmark_cmp
        }
        let required = input.required.contains(&cat);
        if cat == EvidenceCategory::Visual {
            out.push(visual_rollup(input, required));
            continue;
        }
        let tests: Vec<&TestView> =
            input.tests.iter().copied().filter(|t| t.test.kind.category() == cat).collect();
        if tests.is_empty() {
            out.push(KindRollup {
                category: cat,
                status: RollupStatus::Missing,
                counts: RunCounts::default(),
                freshness: None,
                latest: None,
                refs: vec![],
                required,
                summary: "no evidence".into(),
            });
            continue;
        }
        let mut counts = RunCounts::default();
        let mut latest: Option<Timestamp> = None;
        let mut any_fresh_fail = false;
        let mut any_stale_fail = false;
        let mut any_flaky = false;
        let mut any_stale = false;
        let mut all_silent = true;
        let mut worst_fresh: Option<Freshness> = None;
        let mut refs: Vec<EvidenceRef> = Vec::new();
        let mut failing_names: Vec<String> = Vec::new();
        for t in &tests {
            let Some(run) = &t.latest else { continue };
            all_silent = false;
            counts.add(run.status);
            if latest.map(|l| run.timestamp.value > l).unwrap_or(true) {
                latest = Some(run.timestamp.value);
            }
            let fr = freshness(
                run.commit.as_ref(),
                run.timestamp.value,
                run.timestamp.origin,
                input.freshness,
            );
            let is_stale = fr.change == ChangeFreshness::Stale
                || (fr.change == ChangeFreshness::Unknown && fr.time_stale);
            match run.status {
                TestStatus::Failed => {
                    if is_stale {
                        any_stale_fail = true;
                    } else {
                        any_fresh_fail = true;
                    }
                    if failing_names.len() < 5 {
                        failing_names.push(t.test.name.clone());
                    }
                    if refs.len() < 10 {
                        refs.push(EvidenceRef::new(
                            EvidenceKind::Test,
                            t.test.id.as_str(),
                            &t.test.name,
                        ));
                    }
                }
                TestStatus::Flaky => any_flaky = true,
                _ => {}
            }
            if t.flaky {
                any_flaky = true;
            }
            if is_stale && run.status != TestStatus::Skipped {
                any_stale = true;
            }
            // Keep the "worst" freshness for display: stale beats fresh; lower confidence beats higher.
            worst_fresh = match worst_fresh {
                None => Some(fr),
                Some(w) => {
                    let w_stale = w.change == ChangeFreshness::Stale;
                    let f_stale = fr.change == ChangeFreshness::Stale;
                    Some(if f_stale && !w_stale { fr } else { w })
                }
            };
        }
        if all_silent {
            out.push(KindRollup {
                category: cat,
                status: RollupStatus::Silent,
                counts,
                freshness: None,
                latest,
                refs,
                required,
                summary: "linked tests have no recorded runs".into(),
            });
            continue;
        }
        let total = counts.total();
        let (status, summary) = if any_fresh_fail {
            (
                RollupStatus::Fail,
                format!(
                    "{} of {} failing{}",
                    counts.failed,
                    total,
                    if failing_names.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", failing_names.join(", "))
                    }
                ),
            )
        } else if any_stale_fail && any_stale {
            (
                RollupStatus::StaleFail,
                format!("{} failing, last run before the latest change", counts.failed),
            )
        } else if any_flaky {
            (
                RollupStatus::Flaky,
                format!(
                    "{} flaky of {}",
                    counts.flaky.max(tests.iter().filter(|t| t.flaky).count() as u32),
                    total
                ),
            )
        } else if any_stale {
            (
                RollupStatus::Stale,
                format!("{} passing, last validated before the latest change", counts.passed),
            )
        } else if counts.passed + counts.flaky == 0 && counts.skipped > 0 {
            (RollupStatus::Silent, format!("{} skipped", counts.skipped))
        } else {
            (RollupStatus::Pass, format!("{} passing", counts.passed))
        };
        if refs.is_empty() {
            for t in tests.iter().take(5) {
                refs.push(EvidenceRef::new(EvidenceKind::Test, t.test.id.as_str(), &t.test.name));
            }
        }
        out.push(KindRollup {
            category: cat,
            status,
            counts,
            freshness: worst_fresh,
            latest,
            refs,
            required,
            summary,
        });
    }
    out
}

fn visual_rollup(input: &RollupInput, required: bool) -> KindRollup {
    let cat = EvidenceCategory::Visual;
    if input.visuals.is_empty() {
        return KindRollup {
            category: cat,
            status: RollupStatus::Missing,
            counts: RunCounts::default(),
            freshness: None,
            latest: None,
            refs: vec![],
            required,
            summary: "no evidence".into(),
        };
    }
    let mut counts = RunCounts::default();
    let mut latest: Option<Timestamp> = None;
    let mut regressions = 0;
    let mut missing = 0;
    let mut stale = 0;
    let mut worst_fresh: Option<Freshness> = None;
    let mut refs = Vec::new();
    for v in &input.visuals {
        if latest.map(|l| v.timestamp.value > l).unwrap_or(true) {
            latest = Some(v.timestamp.value);
        }
        let fr =
            freshness(v.commit.as_ref(), v.timestamp.value, v.timestamp.origin, input.freshness);
        if fr.change == ChangeFreshness::Stale {
            stale += 1;
        }
        match v.status {
            VisualStatus::Regression => {
                regressions += 1;
                counts.failed += 1;
                if refs.len() < 10 {
                    refs.push(EvidenceRef::new(EvidenceKind::Visual, v.check.as_str(), &v.name));
                }
            }
            VisualStatus::Missing => {
                missing += 1;
                counts.skipped += 1;
            }
            VisualStatus::Current => counts.passed += 1,
            _ => counts.skipped += 1,
        }
        worst_fresh = match worst_fresh {
            None => Some(fr),
            Some(w) => {
                Some(if fr.change == ChangeFreshness::Stale && w.change != ChangeFreshness::Stale {
                    fr
                } else {
                    w
                })
            }
        };
    }
    let (status, summary) = if regressions > 0 {
        (
            RollupStatus::Fail,
            format!("{regressions} visual regression{}", if regressions == 1 { "" } else { "s" }),
        )
    } else if missing > 0 && counts.passed == 0 {
        (RollupStatus::Missing, format!("{missing} without baseline"))
    } else if stale > 0 {
        (
            RollupStatus::Stale,
            format!("{} current, {stale} validated before the latest change", counts.passed),
        )
    } else if counts.passed > 0 {
        (RollupStatus::Pass, format!("{} current", counts.passed))
    } else {
        (RollupStatus::Silent, "no comparable visual results".into())
    };
    KindRollup {
        category: cat,
        status,
        counts,
        freshness: worst_fresh,
        latest,
        refs,
        required,
        summary,
    }
}
