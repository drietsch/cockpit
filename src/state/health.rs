//! Feature health evaluation (§20–§22, §192): rollups per evidence category, then the four
//! states with explicit reasons and a "why" chain.

use crate::config::Config;
use crate::ids::CommitKey;
use crate::model::*;
use jiff::Timestamp;

/// Everything known about one feature's evidence, already rolled up per category.
#[derive(Default, Debug, Clone)]
pub struct Evidence {
    pub rollups: Vec<KindRollup>,
    /// Extra reasons produced while rolling up (flaky tests, duration regressions, drift …).
    pub extra_reasons: Vec<HealthReason>,
}

pub fn evaluate(
    feature: &Feature,
    evidence: &Evidence,
    impl_commit: Option<&CommitKey>,
    impl_at: Option<Timestamp>,
    unresolved: &[UnresolvedRef],
    cfg: &Config,
    now: Timestamp,
) -> FeatureHealth {
    let mut reasons: Vec<HealthReason> = Vec::new();
    let mut chain: Vec<ExplainStep> = Vec::new();
    let importance = feature.importance.as_str();
    let required = cfg.health.required_for(importance);
    let required_cats: Vec<EvidenceCategory> =
        required.iter().filter_map(|r| EvidenceCategory::parse(r)).collect();
    // `test` (or `tests`, `any-test`) in required_evidence: any of unit / integration / e2e.
    let require_any_test =
        required.iter().any(|r| matches!(r.as_str(), "test" | "tests" | "any-test"));
    let broken_kinds: Vec<EvidenceCategory> = cfg
        .health
        .broken_on_failure_kinds
        .iter()
        .filter_map(|k| EvidenceCategory::parse(k))
        .collect();
    let has_any = evidence
        .rollups
        .iter()
        .any(|r| !matches!(r.status, RollupStatus::Missing | RollupStatus::Silent));

    for u in
        unresolved.iter().filter(|u| matches!(&u.from, EntityRef::Feature(f) if f == &feature.id))
    {
        reasons.push(HealthReason {
            reason: AttentionReason::UnresolvedReference,
            severity: Severity::Low,
            text: u.note.clone().unwrap_or_else(|| format!("{} does not resolve", u.raw)),
            evidence: vec![],
            confidence: Confidence::High,
            subject: u.raw.clone(),
            commit: None,
            category: None,
        });
    }

    let state;
    let mut confidence = Confidence::High;
    if !has_any {
        state = HealthState::Unknown;
        chain.push(ExplainStep {
            connective: "given".into(),
            text: "no test, visual or performance evidence is linked to this feature".into(),
            refs: vec![],
        });
        if !matches!(feature.lifecycle, Lifecycle::Planned | Lifecycle::Retired)
            && !required.is_empty()
        {
            let sev = match feature.importance {
                Importance::Critical => Severity::High,
                Importance::High => Severity::Medium,
                _ => Severity::Low,
            };
            reasons.push(HealthReason {
                reason: AttentionReason::MissingRequiredTest,
                severity: sev,
                text: format!(
                    "{importance} feature requires {} evidence; none is linked",
                    required.join(" + ")
                ),
                evidence: vec![],
                confidence: Confidence::High,
                subject: required.join("+"),
                commit: None,
                category: None,
            });
            chain.push(ExplainStep {
                connective: "and".into(),
                text: format!(
                    "a {importance} feature requires {} evidence",
                    required.join(" and ")
                ),
                refs: vec![],
            });
        }
        chain.push(ExplainStep {
            connective: "therefore".into(),
            text: "Unknown — missing evidence is never interpreted as healthy".into(),
            refs: vec![],
        });
    } else {
        let mut broken = false;
        for r in &evidence.rollups {
            let is_required = required_cats.contains(&r.category);
            let label = r.category.label();
            match r.status {
                RollupStatus::Fail => {
                    let gate = broken_kinds.contains(&r.category) || is_required;
                    if gate {
                        broken = true;
                    }
                    reasons.push(HealthReason {
                        reason: if r.category == EvidenceCategory::Visual {
                            AttentionReason::VisualRegression
                        } else {
                            AttentionReason::TestFailed
                        },
                        severity: if gate { Severity::High } else { Severity::Medium },
                        text: format!("{label}: {}", r.summary),
                        evidence: r.refs.clone(),
                        confidence: r
                            .freshness
                            .as_ref()
                            .map(|f| f.confidence)
                            .unwrap_or(Confidence::Medium),
                        subject: r.category.as_str().into(),
                        commit: r.freshness.as_ref().and_then(|f| f.against.clone()),
                        category: Some(r.category),
                    });
                    let when = match r.freshness.as_ref().map(|f| f.change) {
                        Some(ChangeFreshness::Fresh) => " after the latest implementation change",
                        _ => "",
                    };
                    chain.push(ExplainStep {
                        connective: "because".into(),
                        text: format!("{label} evidence is failing{when}: {}", r.summary),
                        refs: r.refs.clone(),
                    });
                }
                RollupStatus::StaleFail => {
                    reasons.push(HealthReason {
                        reason: AttentionReason::TestFailed,
                        severity: Severity::Medium,
                        text: format!("{label}: {} — the failure predates the latest implementation change and may be fixed", r.summary),
                        evidence: r.refs.clone(),
                        confidence: Confidence::Low,
                        subject: r.category.as_str().into(),
                        commit: r.freshness.as_ref().and_then(|f| f.against.clone()),
                        category: Some(r.category),
                    });
                    chain.push(ExplainStep { connective: "but".into(), text: format!("the last {label} failure was recorded before the implementation changed"), refs: r.refs.clone() });
                }
                RollupStatus::Flaky => {
                    reasons.push(HealthReason {
                        reason: AttentionReason::TestFlaky,
                        severity: if feature.importance == Importance::Low {
                            Severity::Low
                        } else {
                            Severity::Medium
                        },
                        text: format!("{label}: {}", r.summary),
                        evidence: r.refs.clone(),
                        confidence: Confidence::High,
                        subject: r.category.as_str().into(),
                        commit: None,
                        category: Some(r.category),
                    });
                }
                RollupStatus::Stale => {
                    reasons.push(HealthReason {
                        reason: if r.category == EvidenceCategory::Visual {
                            AttentionReason::VisualStale
                        } else {
                            AttentionReason::StaleEvidence
                        },
                        severity: if is_required { Severity::Medium } else { Severity::Low },
                        text: format!("{label}: {}", r.summary),
                        evidence: r.refs.clone(),
                        confidence: r
                            .freshness
                            .as_ref()
                            .map(|f| f.confidence)
                            .unwrap_or(Confidence::Medium),
                        subject: r.category.as_str().into(),
                        commit: r.freshness.as_ref().and_then(|f| f.against.clone()),
                        category: Some(r.category),
                    });
                    chain.push(ExplainStep {
                        connective: "but".into(),
                        text: format!("{label} evidence predates the latest implementation change"),
                        refs: r.refs.clone(),
                    });
                }
                RollupStatus::Regression => {
                    reasons.push(HealthReason {
                        reason: AttentionReason::BenchmarkRegression,
                        severity: r.severity.unwrap_or(Severity::Medium),
                        text: format!("{label}: {}", r.summary),
                        evidence: r.refs.clone(),
                        confidence: Confidence::Medium,
                        subject: r.category.as_str().into(),
                        commit: None,
                        category: Some(r.category),
                    });
                    chain.push(ExplainStep {
                        connective: "because".into(),
                        text: r.summary.clone(),
                        refs: r.refs.clone(),
                    });
                }
                RollupStatus::BudgetBreach => {
                    let sev = r.severity.unwrap_or(Severity::High);
                    // Environment-sensitive breaches (Medium) never break a feature.
                    if sev <= Severity::High {
                        broken = true;
                    }
                    reasons.push(HealthReason {
                        reason: AttentionReason::BenchmarkBudgetBreach,
                        severity: sev,
                        text: format!("{label}: {}", r.summary),
                        evidence: r.refs.clone(),
                        confidence: Confidence::High,
                        subject: r.category.as_str().into(),
                        commit: None,
                        category: Some(r.category),
                    });
                    chain.push(ExplainStep {
                        connective: "because".into(),
                        text: r.summary.clone(),
                        refs: r.refs.clone(),
                    });
                }
                RollupStatus::Incomparable => {
                    reasons.push(HealthReason {
                        reason: AttentionReason::BenchmarkIncomparable,
                        severity: Severity::Low,
                        text: format!("{label}: {}", r.summary),
                        evidence: r.refs.clone(),
                        confidence: Confidence::High,
                        subject: r.category.as_str().into(),
                        commit: None,
                        category: Some(r.category),
                    });
                }
                RollupStatus::Missing | RollupStatus::Silent
                    if is_required
                        && !matches!(
                            feature.lifecycle,
                            Lifecycle::Planned | Lifecycle::Retired
                        ) =>
                {
                    reasons.push(HealthReason {
                        reason: if r.category == EvidenceCategory::Visual {
                            AttentionReason::VisualMissing
                        } else {
                            AttentionReason::MissingRequiredTest
                        },
                        severity: if feature.importance == Importance::Critical {
                            Severity::High
                        } else {
                            Severity::Medium
                        },
                        text: format!(
                            "{label} evidence is required for a {importance} feature but {}",
                            if r.status == RollupStatus::Silent {
                                "the latest run produced no results"
                            } else {
                                "none is linked"
                            }
                        ),
                        evidence: r.refs.clone(),
                        confidence: Confidence::High,
                        subject: r.category.as_str().into(),
                        commit: None,
                        category: Some(r.category),
                    });
                    chain.push(ExplainStep {
                        connective: "and".into(),
                        text: format!("{label} evidence is required but missing"),
                        refs: vec![],
                    });
                }
                _ => {}
            }
            if let Some(f) = &r.freshness {
                if !matches!(r.status, RollupStatus::Missing | RollupStatus::Silent) {
                    confidence = confidence.min(f.confidence);
                }
            }
        }
        if require_any_test
            && !evidence.rollups.iter().any(|r| {
                matches!(
                    r.category,
                    EvidenceCategory::Unit | EvidenceCategory::Integration | EvidenceCategory::E2e
                ) && !matches!(r.status, RollupStatus::Missing | RollupStatus::Silent)
            })
            && !matches!(feature.lifecycle, Lifecycle::Planned | Lifecycle::Retired)
        {
            reasons.push(HealthReason {
                reason: AttentionReason::MissingRequiredTest,
                severity: if feature.importance == Importance::Critical {
                    Severity::High
                } else {
                    Severity::Medium
                },
                text: format!(
                    "test evidence (unit, integration or e2e) is required for a {importance} feature but none is linked"
                ),
                evidence: vec![],
                confidence: Confidence::High,
                subject: "test".into(),
                commit: None,
                category: None,
            });
            chain.push(ExplainStep {
                connective: "and".into(),
                text: "test evidence is required but missing".into(),
                refs: vec![],
            });
        }
        // Partial revalidation: some kinds re-ran after the change, others did not.
        let fresh_kinds = evidence
            .rollups
            .iter()
            .filter(|r| {
                r.freshness.as_ref().map(|f| f.change == ChangeFreshness::Fresh).unwrap_or(false)
            })
            .count();
        let stale_kinds = evidence
            .rollups
            .iter()
            .filter(|r| matches!(r.status, RollupStatus::Stale | RollupStatus::StaleFail))
            .count();
        if fresh_kinds > 0 && stale_kinds > 0 {
            reasons.push(HealthReason {
                reason: AttentionReason::IncompleteRevalidation,
                severity: Severity::Medium,
                text: format!("{fresh_kinds} evidence kind{} re-ran after the latest change, {stale_kinds} did not", if fresh_kinds == 1 { "" } else { "s" }),
                evidence: vec![],
                confidence: Confidence::Medium,
                subject: "revalidation".into(),
                commit: impl_commit.cloned(),
                category: None,
            });
        }
        reasons.extend(evidence.extra_reasons.iter().cloned());
        state = if broken {
            HealthState::Broken
        } else if reasons.iter().any(|r| r.severity != Severity::Informational) {
            HealthState::Attention
        } else {
            HealthState::Healthy
        };
        if state == HealthState::Healthy {
            let passing: Vec<String> = evidence
                .rollups
                .iter()
                .filter(|r| r.status == RollupStatus::Pass)
                .map(|r| r.category.label().to_string())
                .collect();
            chain.push(ExplainStep { connective: "given".into(), text: format!("{} evidence is passing and was generated after the latest implementation change", passing.join(", ")), refs: vec![] });
            chain.push(ExplainStep {
                connective: "therefore".into(),
                text: "Healthy".into(),
                refs: vec![],
            });
        } else {
            chain.push(ExplainStep {
                connective: "therefore".into(),
                text: format!(
                    "{}{}",
                    state.label(),
                    if state == HealthState::Attention {
                        " — nothing gating has failed, but the evidence has gaps"
                    } else {
                        ""
                    }
                ),
                refs: vec![],
            });
        }
    }

    let headline = match state {
        HealthState::Unknown => match impl_at {
            Some(t) => format!(
                "No evidence linked yet; implementation last changed {}",
                crate::time::ago(t, now)
            ),
            None => "No evidence linked yet".into(),
        },
        HealthState::Healthy => {
            let n: u32 = evidence.rollups.iter().map(|r| r.counts.passed).sum();
            format!(
                "All linked evidence is passing ({} test{} current)",
                n,
                if n == 1 { "" } else { "s" }
            )
        }
        HealthState::Attention | HealthState::Broken => reasons
            .iter()
            .filter(|r| r.severity != Severity::Informational)
            .map(|r| r.text.clone())
            .next()
            .unwrap_or_else(|| state.label().into()),
    };
    let rank_score = rank(feature, state, &reasons, now, impl_at);
    FeatureHealth {
        state,
        confidence,
        headline,
        reasons,
        rollups: evidence.rollups.clone(),
        chain,
        impl_commit: impl_commit.cloned(),
        impl_committed_at: impl_at,
        evaluated_at: now,
        rank_score,
    }
}

/// Importance-adjusted attention score for ordering (never displayed).
pub fn rank(
    feature: &Feature,
    state: HealthState,
    reasons: &[HealthReason],
    now: Timestamp,
    changed_at: Option<Timestamp>,
) -> i32 {
    let mut s = match feature.importance {
        Importance::Critical => 50,
        Importance::High => 30,
        Importance::Normal => 15,
        Importance::Low => 5,
    };
    s += match state {
        HealthState::Broken => 400,
        HealthState::Attention => 200,
        HealthState::Unknown => 100,
        HealthState::Healthy => 0,
    };
    for r in reasons {
        s += match r.severity {
            Severity::Critical => 40,
            Severity::High => 30,
            Severity::Medium => 20,
            Severity::Low => 10,
            Severity::Informational => 0,
        };
    }
    if reasons.len() >= 2 {
        s += 10;
    }
    if let Some(t) = changed_at {
        let age = now.duration_since(t).as_secs();
        if age < 86_400 {
            s += 20;
        } else if age < 7 * 86_400 {
            s += 10;
        }
    }
    s
}
