use crate::ids::{
    BenchmarkId, BenchmarkRunId, ComponentId, EnvClassId, EnvironmentId, RunId, SourceId,
};
use crate::model::git::CommitRef;
use crate::time::{Origin, Stamped};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Determinism {
    Deterministic,
    SemiDeterministic,
    EnvironmentSensitive,
}

impl Determinism {
    pub fn parse(s: &str) -> Option<Determinism> {
        Some(match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "deterministic" => Determinism::Deterministic,
            "semi-deterministic" | "semi" => Determinism::SemiDeterministic,
            "environment-sensitive" | "env-sensitive" | "wall-clock" => {
                Determinism::EnvironmentSensitive
            }
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Determinism::Deterministic => "deterministic",
            Determinism::SemiDeterministic => "semi-deterministic",
            Determinism::EnvironmentSensitive => "environment-sensitive",
        }
    }
    /// The safe default when a producer does not classify a metric: infer from the unit,
    /// falling back to the class that never gates.
    pub fn infer_from_unit(unit: &str) -> Determinism {
        let u = unit.trim().to_ascii_lowercase();
        if ["count", "calls", "ops", "allocations", "instructions", "bytes", "b", "kb", "mb", "gb"]
            .iter()
            .any(|k| u == *k || u.ends_with(k))
            && !u.contains("/s")
        {
            if u.contains("byte") || u.ends_with('b') {
                return Determinism::SemiDeterministic;
            }
            return Determinism::Deterministic;
        }
        Determinism::EnvironmentSensitive
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    LowerIsBetter,
    HigherIsBetter,
    Informational,
}

impl Direction {
    pub fn parse(s: &str) -> Option<Direction> {
        Some(match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "lower_is_better" | "lower" | "min" => Direction::LowerIsBetter,
            "higher_is_better" | "higher" | "max" => Direction::HigherIsBetter,
            "informational" | "info" | "none" => Direction::Informational,
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Direction::LowerIsBetter => "lower_is_better",
            Direction::HigherIsBetter => "higher_is_better",
            Direction::Informational => "informational",
        }
    }
    pub fn infer_from_unit(unit: &str) -> Direction {
        let u = unit.trim().to_ascii_lowercase();
        if u.contains("/s")
            || u.contains("per s")
            || u.contains("rate")
            || u.contains("throughput")
            || u == "ssim"
        {
            Direction::HigherIsBetter
        } else {
            Direction::LowerIsBetter
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Budget {
    Max(f64),
    Min(f64),
    MaxRelativePct(f64),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricDef {
    pub name: String,
    pub unit: String,
    pub direction: Direction,
    pub direction_origin: Origin,
    pub determinism: Determinism,
    pub determinism_origin: Origin,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Benchmark {
    pub id: BenchmarkId,
    pub source: SourceId,
    pub name: String,
    pub subject: Option<String>,
    pub metric: MetricDef,
    pub budget: Option<Budget>,
    pub component: Option<ComponentId>,
    pub declared_feature_ids: Vec<String>,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfidenceInterval {
    pub level: f64,
    pub lower: f64,
    pub upper: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Statistics {
    pub mean: Option<f64>,
    pub median: Option<f64>,
    pub std_dev: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
    pub n: Option<u32>,
    pub ci: Option<ConfidenceInterval>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeasurementStatus {
    Ok,
    Inconclusive,
    Skipped,
}

impl MeasurementStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            MeasurementStatus::Ok => "ok",
            MeasurementStatus::Inconclusive => "inconclusive",
            MeasurementStatus::Skipped => "skipped",
        }
    }
    pub fn parse(s: &str) -> MeasurementStatus {
        match s {
            "inconclusive" => MeasurementStatus::Inconclusive,
            "skipped" => MeasurementStatus::Skipped,
            _ => MeasurementStatus::Ok,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Measurement {
    pub value: f64,
    pub unit: String,
    pub status: MeasurementStatus,
    pub reason: Option<String>,
    pub statistics: Option<Statistics>,
    pub samples: Option<Vec<f64>>,
    /// A recorded classification / acceptance note ("expected: new index does more work").
    pub accepted: Option<String>,
    /// Relative change reported by the producer (criterion `change/estimates.json`), with
    /// whether it was significant.
    pub reported_change: Option<ReportedChange>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReportedChange {
    pub relative: f64,
    pub significant: Option<bool>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkRun {
    pub id: BenchmarkRunId,
    pub run: RunId,
    pub benchmark: BenchmarkId,
    pub measurement: Measurement,
    pub commit: Option<CommitRef>,
    pub branch: Option<String>,
    pub dirty: Option<bool>,
    pub timestamp: Stamped<Timestamp>,
    pub environment: EnvironmentId,
    pub env_class: EnvClassId,
    pub budget: Option<Budget>,
}
