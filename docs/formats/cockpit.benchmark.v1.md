# `cockpit.benchmark.v1`

A benchmark result is a **document, not a number**: it carries where it came from (commit,
tree state, machine, workload) so Cockpit can decide whether two results may be compared at
all. Results without a resolvable environment are still shown, but never compared.

Cockpit reads this shape directly (`format = "cockpit-benchmark"`) and maps three producer
formats onto it: Guardian `BenchResult` (`guardian-bench`), Criterion.rs output (`criterion`)
and paged measurements (`paged-measurements`).

```json
{
  "schema": "cockpit.benchmark.v1",
  "name": "store",
  "feature_ids": ["FEAT-002"],
  "component": "core.storage",
  "provenance": {
    "commit": "15c882bfefb61a5398bd892e35a03eed1d6b5b34",
    "branch": "main",
    "dirty": false,
    "generated_at": "2026-09-08T14:26:57Z",
    "tool": { "name": "guardian-testkit", "version": "0.4.0" },
    "run": { "id": "17654321", "url": "https://github.com/…/actions/runs/17654321" }
  },
  "build": { "profile": "release", "rustc": "rustc 1.98.0", "lock_hash": "…" },
  "machine": { "os": "macos", "arch": "aarch64", "cpus": 16, "cpu_model": "Apple M3", "runner": "local" },
  "workload": { "backend": "memory", "products": 2000, "batch": 100 },
  "comparability": ["machine.os", "machine.arch", "machine.cpus", "build.profile", "workload"],
  "metrics": [
    { "name": "commit.calls", "unit": "calls", "value": 12, "determinism": "deterministic",
      "direction": "lower_is_better", "budget": { "max": 12 } },
    { "name": "commit(1 field) p50", "unit": "µs", "value": 54.4, "determinism": "environment-sensitive",
      "samples": [51.0, 55.2, 54.4], "statistics": { "n": 200, "p50": 54.4, "p95": 61.0 },
      "status": "ok", "accepted": null }
  ]
}
```

## Fields

| Field | Required | Meaning |
|---|---|---|
| `schema` | yes | literal `cockpit.benchmark.v1` |
| `name` | yes | series name (Guardian: backend; Criterion: group) |
| `feature_ids` (alias `features`) | no | features every metric supports; a metric may add its own `features` |
| `provenance.commit` | strongly recommended | the commit the measured binary was built from; without it freshness is unknown and the result is never green |
| `provenance.generated_at` | recommended | real completion time; absent → the commit's date for checked-in files, else the file's mtime (recorded as `time_source`) |
| `provenance.dirty` / `build.dirty` | no | `true` lowers comparison confidence to *low* |
| `machine`, `build.profile`, `workload` | recommended | the environment fingerprint; `comparability` names the keys that must match for two runs to compare (default: os, arch, cpus, profile, workload) |
| `metrics[].name` (alias `metric`) | yes | metric name |
| `metrics[].subject` | no | sub-series (`dimensions` object is canonicalised into a subject when absent) |
| `metrics[].value` | yes | the measurement; `status: inconclusive` / `skipped` values are stored but never compared |
| `metrics[].unit` | yes | free text; `…/s` and `ssim` infer *higher is better* |
| `metrics[].determinism` | no | `deterministic` · `semi-deterministic` · `environment-sensitive` (default from `[[performance.metrics]]`, else inferred from the unit, else environment-sensitive — the class that never gates) |
| `metrics[].direction` | no | `lower_is_better` · `higher_is_better` · `informational` |
| `metrics[].budget` | no | number (max for lower-is-better, min otherwise) or `{ "max" }`, `{ "min" }`, `{ "max_relative_pct" }` |
| `metrics[].samples`, `statistics` | no | raw samples (capped at 10 000) and summary stats (`mean median std_dev min max p50 p95 p99 n ci`) |
| `metrics[].accepted` (alias `classification`) | no | a recorded acceptance note; an accepted regression is reported, never gating |

## How Cockpit classifies

1. **Baseline** — the latest earlier run in the same environment class (`[performance] baseline = "previous-comparable"`, `"tag:<name>"` or `"commit:<sha>"`); same branch and a clean tree are preferred.
2. No comparable run but other history → **Incomparable** (both environments shown, no percentage). First run → **Unknown**.
3. Budget exceeded → **Budget breach** (breaks the feature unless the metric is environment-sensitive, which only needs attention).
4. Change beyond the class's noise band (`deterministic` 0 %, `semi-deterministic` 10 %, `environment-sensitive` 25 %) in the worse direction → **Regression**: high severity for deterministic, medium for semi-deterministic, *informational* for environment-sensitive — never above informational, however large.
5. Better beyond the band → **Improved**; otherwise **Stable**.

The regression origin is the first run after the baseline from which the value stayed worse;
commits between the prior run and the origin are listed, those touching linked feature paths
first.
