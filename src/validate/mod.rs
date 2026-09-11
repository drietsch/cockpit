//! `cockpit validate` — project policies as enforceable checks with stable codes.

use crate::cli::output::Format;
use crate::cockpit::Cockpit;
use crate::model::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Ignore,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub code: &'static str,
    pub level: Level,
    pub subject: String,
    pub message: String,
    pub policy: &'static str,
}

const CATALOGUE: &[(&str, Level, &str)] = &[
    ("ID001", Level::Error, "feature ids must be unique"),
    ("ID002", Level::Error, "feature ids must match features.id_pattern"),
    ("FM001", Level::Error, "feature/ADR files must parse"),
    ("REF001", Level::Error, "feature → ADR references resolve"),
    ("REF002", Level::Error, "feature dependencies resolve"),
    ("REF003", Level::Error, "test-map / test references name existing features"),
    ("REF004", Level::Warn, "claims name configured components"),
    ("REF005", Level::Error, "ADR superseded_by resolves"),
    ("ADR001", Level::Warn, "ADR status is recognised"),
    ("EV001", Level::Error, "required evidence kinds present per importance"),
    ("EV002", Level::Warn, "evidence generated after the latest implementation change"),
    ("EV003", Level::Warn, "evidence not older than health.max_evidence_age"),
    ("EV004", Level::Error, "critical features are not unknown"),
    ("EV005", Level::Warn, "runs are valid (not all-failed/silent)"),
    ("EV006", Level::Error, "no failing evidence on required kinds"),
    ("SUITE001", Level::Warn, "configured suites publish within expect_every"),
    ("PERF001", Level::Warn, "benchmarks have a comparable baseline"),
    ("PERF002", Level::Error, "no performance budget is breached"),
    ("PERF003", Level::Error, "deterministic regressions are classified (accepted) or fixed"),
    ("PERF004", Level::Warn, "semi-deterministic regressions are classified or fixed"),
    ("CLAIM001", Level::Warn, "claimed components have evidence"),
];

fn level_for(code: &str, cfg: &crate::config::Config) -> Level {
    if code.starts_with("CLAIM") && !cfg.policy.checks.contains_key(code) {
        return match cfg.policy.claims.drift.as_str() {
            "warn" | "warning" => Level::Warn,
            "error" => Level::Error,
            _ => Level::Ignore,
        };
    }
    if code == "PERF001"
        && !cfg.policy.checks.contains_key(code)
        && !cfg.policy.performance.require_comparable_baseline
    {
        return Level::Ignore;
    }
    if let Some(o) = cfg.policy.checks.get(code) {
        return match o.as_str() {
            "ignore" | "off" => Level::Ignore,
            "warn" | "warning" => Level::Warn,
            _ => Level::Error,
        };
    }
    CATALOGUE.iter().find(|(c, _, _)| *c == code).map(|(_, l, _)| *l).unwrap_or(Level::Warn)
}

pub fn run(c: &Cockpit, strict: bool, fmt: &Format) -> anyhow::Result<i32> {
    let st = c.state.load();
    let cfg = &c.located.config;
    let mut findings: Vec<Finding> = Vec::new();
    let mut push = |code: &'static str, subject: String, message: String, policy: &'static str| {
        let level = level_for(code, cfg);
        if level != Level::Ignore {
            findings.push(Finding { code, level, subject, message, policy });
        }
    };

    // Parse errors.
    for w in st.warnings.iter().filter(|w| w.fatal) {
        push("FM001", w.path.to_string(), w.message.clone(), "parse");
    }
    // Ids.
    if let Some(pat) = cfg.features.id_pattern.as_deref().and_then(|p| regex::Regex::new(p).ok()) {
        for f in st.features.values() {
            if !pat.is_match(f.feature.id.as_str()) {
                push(
                    "ID002",
                    f.feature.id.to_string(),
                    format!(
                        "id does not match {}",
                        cfg.features.id_pattern.as_deref().unwrap_or("")
                    ),
                    "features.id_pattern",
                );
            }
        }
    }
    for w in st.warnings.iter().filter(|w| w.message.contains("duplicate id")) {
        push("ID001", w.path.to_string(), w.message.clone(), "features");
    }
    // References.
    for u in &st.unresolved {
        let (code, policy) = match (&u.from, u.target_kind.as_str()) {
            (EntityRef::Feature(_), "adr") => ("REF001", "policy.references"),
            (EntityRef::Feature(_), "feature") => ("REF002", "policy.references"),
            (EntityRef::Test(_), _) => ("REF003", "policy.references"),
            (EntityRef::Adr(_), "adr") => ("REF005", "policy.references"),
            _ => ("REF002", "policy.references"),
        };
        if cfg.policy.references.fail_on_broken_reference || code == "REF003" {
            push(
                code,
                u.from.id_str(),
                u.note.clone().unwrap_or_else(|| format!("{} does not resolve", u.raw)),
                policy,
            );
        }
    }
    for w in st.warnings.iter().filter(|w| w.message.contains("unknown component")) {
        push("REF004", w.path.to_string(), w.message.clone(), "components");
    }
    for a in st.adrs.values().filter(|a| a.adr.status == AdrStatus::Unknown) {
        push(
            "ADR001",
            a.adr.id.to_string(),
            if a.adr.status_raw.is_empty() {
                "no status found".into()
            } else {
                format!("unrecognised status {:?}", a.adr.status_raw)
            },
            "adrs",
        );
    }
    // Evidence per feature.
    for f in st.features.values() {
        let imp = f.feature.importance.as_str();
        let required = cfg.health.required_for(imp);
        if matches!(f.feature.lifecycle, Lifecycle::Planned | Lifecycle::Retired) {
            continue;
        }
        for kind in &required {
            let Some(cat) = EvidenceCategory::parse(kind) else { continue };
            let r = f.health.rollups.iter().find(|r| r.category == cat);
            match r.map(|r| r.status) {
                None | Some(RollupStatus::Missing) | Some(RollupStatus::Silent) => push(
                    "EV001",
                    f.feature.id.to_string(),
                    format!("{imp} feature has no {kind} evidence"),
                    "policy.evidence",
                ),
                Some(RollupStatus::Fail) => push(
                    "EV006",
                    f.feature.id.to_string(),
                    format!("{kind} evidence is failing: {}", r.unwrap().summary),
                    "policy.evidence",
                ),
                _ => {}
            }
        }
        for r in &f.health.rollups {
            if let Some(fr) = &r.freshness {
                if fr.change == ChangeFreshness::Stale
                    && !matches!(r.status, RollupStatus::Missing | RollupStatus::Silent)
                {
                    push(
                        "EV002",
                        f.feature.id.to_string(),
                        format!(
                            "{} evidence predates implementation change {}",
                            r.category.label(),
                            fr.against
                                .as_ref()
                                .map(|c| c.sha.short().to_string())
                                .unwrap_or_default()
                        ),
                        "policy.evidence.require_fresh",
                    );
                } else if fr.time_stale
                    && !matches!(r.status, RollupStatus::Missing | RollupStatus::Silent)
                {
                    push(
                        "EV003",
                        f.feature.id.to_string(),
                        format!(
                            "{} evidence is {} old",
                            r.category.label(),
                            fr.age.map(crate::time::span_compact).unwrap_or_default()
                        ),
                        "health.max_evidence_age",
                    );
                }
            }
        }
        if f.feature.importance == Importance::Critical && f.health.state == HealthState::Unknown {
            push(
                "EV004",
                f.feature.id.to_string(),
                "critical feature has no evidence at all".into(),
                "policy.evidence",
            );
        }
        if strict
            && f.feature.importance == Importance::High
            && f.health.state == HealthState::Unknown
        {
            push(
                "EV004",
                f.feature.id.to_string(),
                "high-importance feature has no evidence at all (strict)".into(),
                "policy.evidence",
            );
        }
        for r in f.health.reasons.iter().filter(|r| r.reason == AttentionReason::UnverifiedClaim) {
            push("CLAIM001", f.feature.id.to_string(), r.text.clone(), "policy.claims");
        }
    }
    for a in st.attention.iter().filter(|a| a.reason == AttentionReason::SuiteSilent) {
        push("SUITE001", a.subject.clone(), a.summary.clone(), "evidence.expect_every");
    }
    for a in st.attention.iter().filter(|a| a.reason == AttentionReason::EvidenceInvalid) {
        push("EV005", a.subject.clone(), a.summary.clone(), "evidence.validity");
    }
    {
        use crate::model::Determinism;
        use crate::state::benchmark_cmp::Classification;
        for b in st.benchmarks.values() {
            let Some(cmp) = &b.comparison else { continue };
            let label = b.label();
            match cmp.classification {
                Classification::Incomparable => {
                    push("PERF001", label, cmp.note.clone(), "policy.performance")
                }
                Classification::BudgetBreach
                    if cmp.determinism != Determinism::EnvironmentSensitive =>
                {
                    push("PERF002", label, cmp.note.clone(), "performance.budget")
                }
                Classification::Regression if cmp.accepted.is_none() => match cmp.determinism {
                    Determinism::Deterministic => {
                        push("PERF003", label, cmp.note.clone(), "performance.policy.deterministic")
                    }
                    Determinism::SemiDeterministic if cmp.gate => push(
                        "PERF004",
                        label,
                        cmp.note.clone(),
                        "performance.policy.semi_deterministic",
                    ),
                    _ => {}
                },
                _ => {}
            }
        }
    }

    if strict {
        for f in findings.iter_mut() {
            if f.level == Level::Warn {
                f.level = Level::Error;
            }
        }
    }
    findings.sort_by(|a, b| {
        b.level
            .cmp(&a.level)
            .then_with(|| a.code.cmp(b.code))
            .then_with(|| a.subject.cmp(&b.subject))
    });
    let errors = findings.iter().filter(|f| f.level == Level::Error).count();
    let warns = findings.iter().filter(|f| f.level == Level::Warn).count();

    if fmt.json {
        fmt.json_out(
            "cockpit.validate.v1",
            &st.project,
            serde_json::json!({
                "strict": strict,
                "errors": errors, "warnings": warns, "ok": errors == 0,
                "findings": findings.iter().map(|f| serde_json::json!({ "code": f.code, "level": match f.level { Level::Error => "error", Level::Warn => "warn", Level::Ignore => "ignore" }, "subject": f.subject, "message": f.message, "policy": f.policy })).collect::<Vec<_>>(),
            }),
        );
        return Ok(if errors > 0 { 1 } else { 0 });
    }
    println!(
        "Project: {} ({} features, {} ADRs, {} tests)",
        fmt.bold(&st.project.name),
        st.counts.features,
        st.counts.adrs,
        st.counts.tests
    );
    println!();
    let ok = |s: &str| println!("{} {s}", fmt.paint("32", "✓"));
    if !findings.iter().any(|f| f.code.starts_with("ID") || f.code == "FM001") {
        ok(&format!("{} feature definitions valid", st.counts.features));
    }
    if !findings.iter().any(|f| f.code.starts_with("REF")) {
        ok("all references resolve");
    }
    if !findings.iter().any(|f| f.code == "ADR001") {
        ok(&format!("{} ADRs valid", st.counts.adrs));
    }
    for f in &findings {
        let glyph = match f.level {
            Level::Error => fmt.paint("31", "✕"),
            Level::Warn => fmt.paint("33", "⚠"),
            Level::Ignore => continue,
        };
        println!(
            "{glyph} {:<8} {:<28} {}   {}",
            f.code,
            crate::cli::status::truncate(&f.subject, 28),
            f.message,
            fmt.dim(f.policy)
        );
    }
    println!();
    println!(
        "{} error{}, {} warning{} → exit {}",
        errors,
        if errors == 1 { "" } else { "s" },
        warns,
        if warns == 1 { "" } else { "s" },
        if errors > 0 { 1 } else { 0 }
    );
    Ok(if errors > 0 { 1 } else { 0 })
}
