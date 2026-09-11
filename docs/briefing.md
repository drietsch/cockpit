# Project Cockpit

## Master Product, Architecture & UX Specification

**Status:** Concept / Product Specification  
**Primary implementation language:** Rust  
**Primary deployment model:** Single self-contained binary  
**Primary usage model:** Local-first engineering cockpit  
**Primary audience:** Engineers building and maintaining software, especially through highly agentic development workflows

---

# 1. Executive Summary

Project Cockpit is a self-contained engineering-state application for software projects.

It combines:

- feature specifications
- architecture decisions
- automated tests
- end-to-end tests
- test artifacts
- screenshots
- visual regression results
- videos
- Playwright traces
- benchmarks
- performance budgets
- coverage
- Git history
- GitHub metadata
- CI results
- documentation
- acceptance criteria

into one coherent model of the current state of an application.

The product exists to answer:

> **Where does this application stand right now?**

More specifically:

> **What is healthy, what is broken, what deserves attention, what changed recently, and what evidence supports those conclusions?**

Project Cockpit is not primarily a documentation viewer.

It is not primarily a test report.

It is not primarily a Git browser.

It is not primarily a performance dashboard.

It combines all of those sources into a higher-level interpretation of engineering state.

The user should be able to enter any repository and run:

```bash id="d1in3y"
cockpit serve
```

and receive a rich local web application.

The distributed product should consist of one executable:

```text id="y0tvbb"
cockpit
```

The user should not need to install or operate:

```text id="wzruoa"
Node.js
npm
Docker
PostgreSQL
Redis
Python
a web server
a separate frontend
a remote database
a cloud account
```

The entire application—including the web UI—should be embedded in the Rust binary.

---

# 2. The Fundamental Problem

Modern software projects contain more evidence about their state than ever before.

A project may have:

```text id="9qeswo"
42 feature specifications
27 ADRs
1,300 unit tests
180 integration tests
90 E2E tests
130 visual baselines
30 performance benchmarks
hundreds of screenshots
dozens of videos
Playwright traces
Git history
CI history
coverage reports
```

Yet answering a simple question can still be surprisingly difficult:

> **Is the project okay?**

The engineer often has to inspect several disconnected systems:

```text id="azcq2o"
README
↓
test report
↓
Playwright report
↓
benchmark output
↓
GitHub Actions
↓
Git log
↓
ADR directory
↓
coverage report
```

and manually construct a mental model.

That problem becomes significantly worse with agentic development.

Agents can produce large amounts of:

- code
- tests
- ADRs
- commits
- screenshots
- reports
- benchmark results

very quickly.

The bottleneck increasingly becomes:

> **human comprehension of system state**

Project Cockpit exists to solve that bottleneck.

---

# 3. Primary Product Objective

The defining product question is:

> **If I have not seen this project for two weeks, can I understand its engineering state within 30 seconds?**

The application should support three levels of comprehension.

## 3.1 Five-second understanding

Within approximately five seconds:

```text id="40wet6"
Is the project broadly okay?

Is something broken?

How many important things need attention?
```

## 3.2 Thirty-second understanding

Within approximately thirty seconds:

```text id="jjmhzk"
What needs attention?

Which features are affected?

What changed?

How recent is the evidence?

What should I investigate first?
```

## 3.3 Five-minute investigation

Within a few minutes:

```text id="pxnp50"
Why did this state occur?

Which commit correlates with it?

Which tests or benchmarks prove it?

Which ADR explains the implementation?

Can I inspect the screenshot/video/trace/raw measurement?
```

The UI architecture should follow this progression.

---

# 4. Core Information Hierarchy

Project Cockpit should always prefer:

```text id="xtffwu"
STATE
   ↓
ATTENTION
   ↓
CHANGE
   ↓
IMPACTED FEATURE
   ↓
REASON
   ↓
EVIDENCE
   ↓
RAW ARTIFACT
```

over:

```text id="bo9qif"
RAW ARTIFACT
↓
RAW ARTIFACT
↓
RAW ARTIFACT
↓
USER FIGURES OUT WHAT IT MEANS
```

This principle should influence every major screen.

---

# 5. Product Mental Model

The user should think about Cockpit as:

> **the engineering instrument panel for the repository**

The cockpit does not own the airplane.

It does not build the airplane.

It does not replace the aircraft's individual systems.

It observes, correlates, summarizes, and signals.

The software equivalent is:

```text id="b1albl"
repository
    │
    ├── features
    ├── architecture
    ├── source code
    ├── tests
    ├── benchmarks
    ├── artifacts
    └── Git history
          │
          ▼
       Cockpit
          │
          ├── state
          ├── attention
          ├── change
          ├── evidence
          └── explanation
```

---

# 6. Product Principles

## 6.1 State before data

Cockpit should interpret evidence before showing raw evidence.

## 6.2 Attention before metrics

What needs action is usually more important than totals.

## 6.3 Feature-centric over tool-centric

The primary question is:

> Is Search healthy?

not:

> What does Playwright say?

## 6.4 Change is first-class

Project state without change context is incomplete.

## 6.5 Unknown must remain unknown

Missing evidence must never be interpreted as healthy.

## 6.6 Evidence must be auditable

Every state should be explainable.

## 6.7 Freshness matters

Passing evidence from before the implementation changed may no longer be valid.

## 6.8 Local-first

The core system should work entirely offline.

## 6.9 Repository remains authoritative

Cockpit derives state. It does not become the primary source of project truth.

## 6.10 Human and agent symmetry

Humans use the GUI.

Agents use CLI/JSON/API.

Both operate on the same state model.

## 6.11 Visualization with purpose

Charts should explain trends, regressions, distributions, or correlations.

Charts should never exist merely because dashboards are expected to have charts.

---

# 7. Core User Scenarios

## 7.1 Returning to an old project

The user opens a project they have not touched in several weeks.

Cockpit should immediately answer:

```text id="imw7ha"
1 feature broken
2 features need attention
41 healthy
3 have incomplete evidence

17 commits since last release

Search performance regressed
PDF Export E2E is failing
Payments visual tests are stale
```

## 7.2 Supervising an agent

An agent is implementing a feature.

Cockpit updates live:

```text id="65np2e"
10:14 implementation changed
10:15 unit tests completed
10:16 E2E completed
10:17 screenshots generated
10:18 performance benchmark completed
```

The user can observe the engineering state becoming complete.

## 7.3 Investigating a regression

The user notices:

```text id="cb082h"
Search
⚠ Performance +18%
```

They click it.

Cockpit explains:

```text id="v2ia3k"
Search API p95

112 ms → 132 ms

Regression first observed after:
a41f92c — Replace search index
```

The user then inspects:

```text id="0xs85l"
benchmark history
changed files
related ADR
raw benchmark
```

## 7.4 Release readiness

The user selects:

```text id="z2p7bp"
Compare against v1.12.0
```

Cockpit shows:

```text id="s29aih"
4 new features
84 additional tests
2 new ADRs
1 performance regression
0 broken visual tests
2 incomplete validations
```

---

# 8. Distribution Model

Project Cockpit should ship as platform-specific standalone executables.

Examples:

```text id="lyjgzx"
cockpit-linux-x86_64
cockpit-linux-aarch64
cockpit-macos-aarch64
cockpit-macos-x86_64
cockpit-windows-x86_64.exe
```

Possible installation:

```bash id="ezbry2"
cargo install cockpit
```

or release binary download.

Runtime dependency expectations:

```text id="mhbafi"
Rust runtime          none
Node.js               none
external database     none
external web server   none
internet connection   optional
```

---

# 9. Application Modes

Cockpit should eventually support several operating modes.

## 9.1 Serve mode

```bash id="lukzyq"
cockpit serve
```

Starts the web application.

## 9.2 Watch mode

```bash id="skak6l"
cockpit serve --watch
```

Watches project files and automatically rescans relevant state.

## 9.3 Scan mode

```bash id="hhnlxh"
cockpit scan
```

Parses project state and refreshes the derived index.

## 9.4 Status mode

```bash id="rsvd1e"
cockpit status
```

Returns a terminal summary.

## 9.5 Validation mode

```bash id="3i6jtq"
cockpit validate
```

Checks structural and policy constraints.

## 9.6 Query mode

```bash id="fqnk1d"
cockpit query 'features where state = broken'
```

Provides structured access to project state.

## 9.7 JSON mode

Most commands should support:

```bash id="5afz12"
--json
```

for machine consumption.

---

# 10. Repository as the Source of Truth

Cockpit should assume the repository is authoritative.

Example:

```text id="nlvh09"
project/
├── cockpit.toml
├── docs/
├── src/
├── tests/
├── artifacts/
└── .git/
```

Cockpit reads the project.

It does not require project state to be imported into a proprietary database.

---

# 11. Suggested Repository Structure

```text id="36r12m"
project/
│
├── cockpit.toml
│
├── docs/
│   ├── features/
│   │   ├── FEAT-001-login.md
│   │   ├── FEAT-002-search.md
│   │   └── FEAT-003-pdf-export.md
│   │
│   ├── adr/
│   │   ├── ADR-001-database.md
│   │   ├── ADR-002-auth.md
│   │   └── ADR-003-search-index.md
│   │
│   └── architecture/
│       ├── overview.md
│       └── data-flow.md
│
├── artifacts/
│   ├── tests/
│   │   ├── unit.xml
│   │   └── integration.xml
│   │
│   ├── playwright/
│   │   ├── results.json
│   │   ├── screenshots/
│   │   ├── videos/
│   │   └── traces/
│   │
│   ├── performance/
│   │   ├── criterion.jsonl
│   │   └── iai.json
│   │
│   └── coverage/
│       └── lcov.info
│
└── src/
```

---

# 12. Configuration Philosophy

Configuration should be minimal.

Cockpit should rely on conventions where possible.

Example:

```toml id="oh31rj"
[project]
id = "invoice-app"
name = "Invoice App"

[features]
path = "docs/features"

[adrs]
path = "docs/adr"

[tests.unit]
format = "junit"
glob = "artifacts/tests/unit/*.xml"

[tests.e2e]
format = "playwright"
glob = "artifacts/playwright/results.json"

[visual]
path = "artifacts/playwright"

[performance]
glob = "artifacts/performance/**"

[coverage]
format = "lcov"
path = "artifacts/coverage/lcov.info"

[health]
max_evidence_age = "7d"
```

Ideally, conventionally structured projects should require little or no configuration.

---

# 13. Stable Entity IDs

Every important long-lived engineering concept should have a stable identifier.

Recommended namespaces:

```text id="wj2vw9"
FEAT-012
ADR-007
TEST-183
E2E-081
VIS-104
PERF-019
RUN-18329
```

Stable IDs provide:

- durable references
- easy linking
- robust agent interaction
- reliable graph relationships
- usable URLs
- long-lived history

---

# 14. Engineering State Graph

The core internal model should be graph-like.

Example:

```text id="o1vhrh"
                         ADR-022
                            │
                            ▼
                        FEAT-019
                          Search
                            │
        ┌───────────────────┼──────────────────┐
        │                   │                  │
        ▼                   ▼                  ▼
     TEST-88              E2E-12             PERF-19
                             │                  │
                     ┌───────┼───────┐          │
                     ▼       ▼       ▼          │
                   image   video   trace         │
                                                │
                             ▲                  │
                             │                  │
                           COMMIT───────────────┘
                             │
                             ▼
                             PR
                             │
                   ┌─────────┴────────┐
                   ▼                  ▼
                 ISSUE                CI
```

---

# 15. Core Entity Types

The normalized model should eventually support:

```text id="yhowdt"
Project
Feature
ADR
Test
TestRun
Benchmark
BenchmarkRun
Measurement
VisualEvidence
Artifact
Commit
PullRequest
Issue
CiRun
CoverageResult
AttentionItem
Policy
Environment
```

---

# 16. Feature Model

A feature is the main user-facing abstraction.

Possible model:

```rust id="8atcgo"
struct Feature {
    id: FeatureId,
    title: String,
    description: Option<String>,
    status: FeatureLifecycleStatus,
    importance: Importance,
    owners: Vec<String>,
    tags: Vec<String>,
    paths: Vec<PathPattern>,
    adr_ids: Vec<AdrId>,
    dependencies: Vec<FeatureId>,
    metadata: Metadata,
}
```

---

# 17. Feature Markdown

Example:

```markdown id="jdiajv"
---
id: FEAT-019
title: Search
status: stable
importance: high

owners:
  - search

tags:
  - user-facing
  - core

paths:
  - src/search/**
  - src/index/**

adrs:
  - ADR-016
  - ADR-022
---

# Search

Allows users to search invoices, clients, and projects.

## Acceptance criteria

- Search results appear within 200 ms for standard workloads.
- Matching should tolerate minor spelling errors.
- Search results must respect user permissions.
```

---

# 18. Feature Lifecycle vs Feature Health

Cockpit should distinguish feature lifecycle from feature health.

Lifecycle:

```text id="09erqd"
planned
experimental
beta
stable
deprecated
retired
```

Health:

```text id="0hgzj3"
healthy
attention
broken
unknown
```

A beta feature can still be healthy.

A stable feature can be broken.

These concepts should not be conflated.

---

# 19. Feature Importance

Optional importance levels:

```text id="u7gay2"
critical
high
normal
low
```

Importance should influence attention ordering.

Example:

```text id="62et0e"
Critical authentication E2E failure
```

should outrank:

```text id="tui6t9"
Low-priority experimental UI visual baseline stale
```

even if both are technically failures.

---

# 20. Health States

Use four primary health states.

## Healthy

Evidence indicates expected state.

## Attention

Something is degraded, incomplete, unusual, or at risk.

## Broken

A meaningful requirement or critical evidence has failed.

## Unknown

Cockpit lacks enough evidence to make a reliable statement.

---

# 21. State Computation

A feature's state should not normally be manually specified.

Instead:

```text id="0hlw6v"
FeatureState =
    evaluate(
        tests,
        visual evidence,
        benchmarks,
        freshness,
        policies,
        references,
        change history
    )
```

Conceptually:

```rust id="l74idq"
fn evaluate_feature(feature: &Feature, evidence: &Evidence) -> FeatureHealth
```

The result should include:

```rust id="0w5i23"
struct FeatureHealth {
    state: HealthState,
    confidence: Confidence,
    reasons: Vec<HealthReason>,
    evaluated_at: Timestamp,
}
```

---

# 22. Health Must Be Explainable

Never expose only:

```text id="pg5aru"
Search: Attention
```

Expose:

```text id="8z1a23"
Search
⚠ Needs Attention

Reasons:
- Search API p95 increased 17.9%
- Regression was first observed after a41f92c
- Performance remains inside the 150 ms budget
```

Health should always be auditable.

---

# 23. Attention Items

The attention system should be an explicit model.

```rust id="zz1r80"
struct AttentionItem {
    id: AttentionId,
    severity: Severity,
    state: AttentionState,
    feature_id: Option<FeatureId>,
    title: String,
    summary: String,
    reason: AttentionReason,
    evidence: Vec<EvidenceRef>,
    first_seen: Timestamp,
    last_seen: Timestamp,
}
```

Possible reasons:

```text id="dgv8x1"
test_failed
test_flaky
visual_regression
visual_missing
visual_stale
benchmark_regression
benchmark_budget_breach
benchmark_incomparable
coverage_regression
missing_required_test
stale_evidence
unresolved_reference
incomplete_revalidation
ci_failure
policy_failure
```

---

# 24. Attention Severity

Possible severity levels:

```text id="ddj4e2"
critical
high
medium
low
informational
```

However, the UI should not necessarily display severity labels everywhere.

The first-level user mental model should remain:

```text id="not8m6"
Broken
Needs Attention
Unknown
```

Severity mainly helps ordering.

---

# 25. Attention Ranking

A ranking function may consider:

```text id="30ozqg"
feature importance
health severity
confidence
change recency
user-facing impact
whether newly introduced
number of affected checks
budget breach magnitude
evidence age
production criticality
```

Example scoring concept:

```text id="t0jbfg"
critical feature             +50
broken                       +40
introduced today             +20
high confidence              +10
multiple independent signals +10
```

The exact formula should remain internal.

The UI should not expose a meaningless score.

---

# 26. Evidence Model

Evidence should be normalized.

Possible abstraction:

```rust id="iaeiqf"
enum Evidence {
    Test(TestEvidence),
    Visual(VisualEvidence),
    Benchmark(BenchmarkEvidence),
    Coverage(CoverageEvidence),
    Documentation(DocumentEvidence),
    Ci(CiEvidence),
}
```

Every evidence item should ideally include:

```text id="dcv6uu"
source
status
commit
timestamp
environment
tool
tool version
artifact references
```

---

# 27. Provenance

Evidence without provenance becomes difficult to trust.

Example:

```yaml id="gf5rf7"
commit: a41f92c
branch: main

generated_at: 2026-09-11T08:42:13Z

tool:
  name: playwright
  version: 1.x

environment:
  id: ci-linux-x64

run:
  id: 18329
```

Cockpit should retain as much provenance as reasonably available.

---

# 28. Evidence Freshness

Cockpit should evaluate freshness in two ways.

## 28.1 Time freshness

Example:

```text id="j1eayt"
last run 18 days ago
```

## 28.2 Change freshness

More important:

```text id="83idq2"
latest implementation commit occurred after latest test run
```

Change freshness should normally outrank time freshness.

---

# 29. Revalidation Semantics

The strongest model is:

> Evidence is valid if it was generated after the latest relevant implementation change.

Example:

```text id="okcfis"
Latest Search implementation change
a41f92c

Unit
✓ generated after a41f92c

E2E
✓ generated after a41f92c

Visual
⚠ generated before a41f92c

Performance
✓ generated after a41f92c
```

This creates a very intuitive trust model.

---

# 30. Determining Relevant Changes

Cockpit can associate commits with features using several strategies.

Priority:

```text id="a6e5cw"
1. explicit feature reference in commit
2. explicit feature reference in PR
3. path ownership
4. test ownership
5. module/package ownership
6. heuristic inference
```

Heuristic relationships should be visually distinguishable from explicit relationships if necessary.

---

# 31. Git Integration

Git should be a core dependency.

Cockpit should understand:

```text id="shgxnq"
repository root
branch
HEAD
working tree status
commit history
tags
authors
timestamps
messages
changed files
diff stats
merge relationships
```

Git is not merely another page.

It provides temporal context for engineering state.

---

# 32. Commit Model

Conceptual structure:

```rust id="8htfqe"
struct Commit {
    id: CommitId,
    short_id: String,
    summary: String,
    author: String,
    timestamp: Timestamp,
    parents: Vec<CommitId>,
    changed_files: Vec<ChangedFile>,
    feature_ids: Vec<FeatureId>,
}
```

---

# 33. Commit-to-Feature Mapping

Example explicit reference:

```text id="jsgdtp"
feat(search): replace index backend [FEAT-019]
```

Cockpit extracts:

```text id="2x3lqr"
a41f92c → FEAT-019
```

Fallback:

```yaml id="stjrs4"
paths:
  - src/search/**
  - src/index/**
```

Then:

```text id="fskvs8"
commit touches src/search/index.rs
         ↓
likely affects FEAT-019
```

---

# 34. Recent Activity Model

Cockpit should not display a generic chronological Git feed.

It should generate engineering events.

Examples:

```text id="jm00j2"
Feature changed
Feature validated
Feature became broken
Feature recovered
Performance regression detected
Visual regression detected
ADR accepted
New feature introduced
Evidence became stale
```

These events can produce a useful timeline.

---

# 35. Engineering Timeline

Example:

```text id="vz0gk7"
09:12  a41f92c Search index changed
09:16  unit tests passed
09:18  E2E passed
09:20  visual tests passed
09:23  benchmark completed
09:23  ⚠ performance regression detected
```

This is more informative than presenting separate reports.

---

# 36. GitHub Enrichment

GitHub should remain optional.

Potential relationships:

```text id="q6ca71"
commit → pull request
pull request → issue
pull request → reviewer
pull request → CI
issue → feature
```

Possible data:

```text id="xld08h"
PR title
PR status
merged timestamp
review state
GitHub checks
issue references
author
labels
```

Core Cockpit functionality must remain available offline.

---

# 37. Test Model

Normalize test results regardless of framework.

```rust id="rnrq5w"
struct Test {
    id: TestId,
    name: String,
    kind: TestKind,
    feature_ids: Vec<FeatureId>,
    source: Option<PathBuf>,
}

struct TestRun {
    id: TestRunId,
    test_id: TestId,
    status: TestStatus,
    duration: Duration,
    commit: Option<CommitId>,
    environment: Option<EnvironmentId>,
    timestamp: Timestamp,
    artifacts: Vec<ArtifactId>,
}
```

---

# 38. Test Kinds

Possible kinds:

```text id="ac1n5p"
unit
integration
E2E
contract
visual
smoke
security
performance-related
```

Cockpit should not impose a framework-specific taxonomy.

---

# 39. Test History

Individual tests should accumulate history.

This enables:

```text id="gugzdb"
flakiness detection
duration trends
failure frequency
first failing commit
last successful run
```

Example:

```text id="ciqi3u"
latest 20 runs

✓ ✓ ✓ ✓ ✕ ✓ ✓ ✓ ✓ ✕ ✓ ✓ ✓ ✓ ✓ ✓ ✕ ✓ ✓ ✓
```

---

# 40. Flakiness

Flakiness should be treated separately from hard failure.

Possible model:

```text id="ez5tc8"
Healthy
Flaky
Failing
Unknown
```

A flaky critical test can create an attention item.

Possible rule:

```text id="ov86zi"
3 failures in last 20 runs
→ flaky
```

The exact policy should be configurable.

---

# 41. Test Duration

Test duration is useful state.

Cockpit can detect:

```text id="t5wdd6"
Checkout E2E

6.1s → 8.0s

+31%
```

Even though the test passes.

This can become a low- or medium-priority performance attention item.

---

# 42. Playwright Integration

Playwright should have first-class integration.

Useful inputs:

```text id="1hx9hm"
JSON result data
screenshots
visual baselines
visual diff images
videos
traces
HTML report
browser metadata
viewport metadata
```

---

# 43. Playwright Test Detail

Example:

```text id="xgi2d2"
E2E-RESET-04

Password reset works from login screen

✕ Failed

Feature
FEAT-012 Password Reset

Browser
Chromium

Viewport
1440 × 900

Commit
a13f92d

Duration
4.31 s

Failure
Expected confirmation message was not visible.
```

Artifacts:

```text id="vhzkjy"
[ Screenshot ]
[ Video ]
[ Trace ]
[ Playwright Report ]
```

---

# 44. Visual Testing Model

Visual evidence should be first-class.

```rust id="rvhcod"
struct VisualEvidence {
    id: VisualEvidenceId,
    feature_ids: Vec<FeatureId>,
    test_id: Option<TestId>,
    browser: Option<String>,
    viewport: Option<Viewport>,
    baseline: ArtifactId,
    actual: ArtifactId,
    diff: Option<ArtifactId>,
    changed_pixels: Option<u64>,
    difference_ratio: Option<f64>,
    status: VisualStatus,
    commit: Option<CommitId>,
    timestamp: Timestamp,
}
```

---

# 45. Visual Regression Detail

Primary view:

```text id="no1tmu"
Dashboard / Main State

Chromium
1440 × 900

Expected        Actual          Diff

[image]         [image]         [image]

Difference
1.8%

Changed pixels
14,218
```

Secondary information:

```text id="54zzfr"
Feature
Commit
Test
Timestamp
Browser
Viewport
Artifact path
```

---

# 46. Screenshot Viewer UX

The screenshot viewer should support:

```text id="5e2v30"
fit-to-screen
100% pixels
zoom
pan
expected/actual/diff switching
side-by-side mode
overlay comparison
keyboard navigation
metadata drawer
```

The viewer should feel like a specialized engineering tool, not a generic lightbox.

---

# 47. Video UX

Failure videos should display inline.

Context should remain visible:

```text id="p2r2nu"
Feature
Test
Commit
Browser
Failure reason
Timestamp
```

The user should not lose engineering context when watching the artifact.

---

# 48. Performance as Evidence

Performance should be modeled as engineering evidence rather than separate analytics.

A benchmark should answer:

```text id="n7idqr"
How fast is it now?
What was the baseline?
Did it change?
Is that change statistically meaningful?
Is it inside the budget?
When did it change?
Which implementation change correlates with it?
```

---

# 49. Benchmark Definition

Conceptual model:

```rust id="osqk6p"
struct Benchmark {
    id: BenchmarkId,
    name: String,
    feature_ids: Vec<FeatureId>,
    metric: MetricDefinition,
    budget: Option<Budget>,
    regression_policy: RegressionPolicy,
    expected_environment: Option<EnvironmentClass>,
}
```

---

# 50. Benchmark Run

```rust id="poiiqw"
struct BenchmarkRun {
    id: BenchmarkRunId,
    benchmark_id: BenchmarkId,
    commit: CommitId,
    timestamp: Timestamp,
    environment: EnvironmentFingerprint,
    samples: Vec<Measurement>,
    statistics: BenchmarkStatistics,
}
```

---

# 51. Benchmark Comparison

```rust id="jmskdn"
struct BenchmarkComparison {
    benchmark_id: BenchmarkId,
    current_run: BenchmarkRunId,
    baseline_run: BenchmarkRunId,

    absolute_change: f64,
    relative_change: f64,

    significance: Option<StatisticalSignificance>,
    classification: BenchmarkClassification,
}
```

---

# 52. Benchmark Classification

Suggested classifications:

```text id="om4bi1"
Healthy
Improved
Regression
Budget Breach
Unknown
Incomparable
```

Improvement should usually be positive information but not dominate attention.

---

# 53. Performance Metrics

Cockpit should support generic units and directions.

Examples:

```text id="t4bdrf"
latency                 lower is better
render time             lower is better
allocations             lower is better
memory                   lower is better
binary size              lower is better

throughput               higher is better
requests/sec             higher is better
operations/sec           higher is better
cache hit rate           higher is better
```

Metric definition should know which direction is desirable.

---

# 54. Supported Benchmark Metrics

Potential metrics include:

```text id="4wbaba"
mean latency
median latency
p50
p95
p99
throughput
operations/sec
requests/sec
memory peak
memory retained
allocation count
allocation bytes
CPU instructions
branch instructions
branch misses
cache misses
page-load duration
first contentful paint
largest contentful paint
bundle size
binary size
database query duration
render duration
startup time
```

---

# 55. Benchmark Budgets

A budget should be an explicit engineering expectation.

Example:

```yaml id="am6zgo"
metric: search.p95
max: 150ms
```

Cockpit can distinguish:

```text id="ws92rl"
132 ms
+18%

Budget: 150 ms

⚠ Regression
```

from:

```text id="tnubdl"
181 ms

Budget: 150 ms

✕ Budget Breach
```

---

# 56. Regression Policy

A regression policy might specify:

```yaml id="e2w2xc"
max_relative_change: 10%
noise_threshold: 3%
require_statistical_significance: true
```

Different metrics may have different policies.

---

# 57. Benchmark Baselines

Supported baseline modes:

```text id="gjxoxu"
previous comparable run
latest successful main run
latest release
specified Git tag
specified commit
rolling median
golden baseline
```

Default should probably be:

> latest comparable successful run on the same primary branch and environment class

---

# 58. Environment Fingerprints

Benchmark comparisons are only meaningful under reasonably comparable conditions.

Environment fingerprint example:

```yaml id="xu26yq"
id: ci-linux-x64

os: linux
arch: x86_64

cpu:
  model: ...
  cores: 4

compiler:
  rustc: ...

profile: release

runner:
  provider: github-actions

benchmark_tool:
  name: criterion
  version: ...
```

---

# 59. Environment Classes

Cockpit can group runs:

```text id="39eix6"
ci-linux-x64
local-m4-max
release-runner
local-linux-amd
```

By default:

```text id="nxkcqp"
only compare benchmark runs inside compatible environment classes
```

---

# 60. Incomparable Benchmarks

Cockpit should explicitly show:

```text id="y72z6z"
? Incomparable

Current
91 ms
local-m4

Baseline
132 ms
ci-linux-x64
```

Do not calculate a misleading regression percentage.

---

# 61. Statistical Significance

Cockpit should preserve upstream statistical interpretation.

Example:

```text id="81rq16"
112.0 ms → 113.1 ms

+0.98%

No significant change
inside benchmark noise threshold
```

versus:

```text id="7kn0ex"
112.0 ms → 132.0 ms

+17.9%

Statistically significant regression
```

---

# 62. Criterion Integration

Criterion should be a native importer for Rust projects.

Cockpit should normalize Criterion output into the generic benchmark model.

Cockpit should avoid becoming tightly coupled to private internal report files.

The importer layer should shield the rest of Cockpit from tool-specific details.

---

# 63. Iai-Callgrind Integration

Iai-Callgrind is valuable for deterministic lower-level metrics.

Possible results:

```text id="twmlmx"
CPU instructions
memory reads
memory writes
cache misses
branches
branch misses
```

This can complement wall-clock benchmarks.

Example:

```text id="l8vocg"
Search

Wall clock
132 ms
+18%

Instructions
+17.2%

Cache misses
+32.1%

Conclusion:
The implementation is doing materially more work.
```

---

# 64. Performance Correlation with Git

This should become one of Cockpit's flagship features.

Example trend:

```text id="yqiytc"
Search API p95

150ms ─────────────────────────── budget
140ms ┤
130ms ┤                            ●
120ms ┤      ●   ●   ●
110ms ┤  ●
100ms ┤
      └────────────────────────────────
       Aug 28             Sep 11

                     │
                  a41f92c
             Replace search index
```

Clicking the marker shows:

```text id="j2o27c"
a41f92c

Replace search index

Affected files:
src/search/index.rs
src/search/query.rs

Affected feature:
FEAT-019 Search

Previous comparable run
112 ms

First run after commit
132 ms

Difference
+17.9%
```

---

# 65. Performance History

Benchmark history should retain sufficient data for:

```text id="54lvze"
trend charts
baseline comparisons
release comparisons
regression origin
rolling statistics
environment grouping
```

History should be preserved through CI artifacts or Cockpit interchange files where possible.

---

# 66. Performance Detail UX

Top-level information:

```text id="pczm8m"
Search API p95                        PERF-019

⚠ Regression

132 ms

+17.9%
vs previous main

Budget
150 ms
```

Primary chart follows.

Then:

```text id="baq6xq"
Regression first observed
a41f92c

Confidence
High

Environment
ci-linux-x64

Baseline
112 ms
```

Advanced tabs:

```text id="d61zqu"
History
Distribution
Samples
Environment
Commits
Flamegraph
Raw
```

---

# 67. Homepage Performance Section

Project-wide performance should focus on change.

Example:

```text id="23c552"
PERFORMANCE CHANGES

Search API p95          132 ms      +18.0% ⚠
PDF render              1.82 s       +7.4% ⚠
Auth middleware          0.42 ms      +1.1%
Serialization             81 µs       -6.2% ✓
```

Default ordering:

```text id="0mqz71"
budget breach
largest meaningful regression
other regressions
stable metrics
improvements
```

---

# 68. Charts: General Philosophy

Cockpit should be visualization-rich.

But charts are not decoration.

Every chart must answer a concrete question.

Good:

```text id="wbfm9c"
When did latency worsen?
```

Bad:

```text id="2atqn0"
Let's add a pie chart because the page looks empty.
```

---

# 69. Chart Types

## Line charts

Use for:

```text id="r4xdqk"
performance history
test duration history
coverage history
memory trends
```

## Horizontal bars

Use for:

```text id="e87dfu"
largest regressions
slowest tests
features with most gaps
```

## Scatter plots

Use for:

```text id="a72vyn"
measurement distributions
correlation
noisy benchmarks
```

## Box plots

Use for:

```text id="zfvs99"
benchmark sample distributions
run variability
```

## Run strips

Use for:

```text id="2qv9xy"
test success/failure history
flakiness
```

## Sparklines

Use in:

```text id="0hdp28"
feature table
benchmark list
test list
```

---

# 70. Chart Design Rules

Charts should use:

```text id="ww0i8f"
minimal grid
high data-to-ink ratio
subtle axes
clear units
precise tooltips
visible budget lines
commit markers
release markers
```

Avoid:

```text id="rgonqb"
3D charts
excessive gradients
large legends
decorative shadows
animation on every load
```

---

# 71. Chart Interactions

Useful interactions:

```text id="3fory5"
hover tooltip
zoom selection
drag pan
click commit marker
click data point
change baseline
show/hide comparison
inspect run
```

Deep analysis should remain optional.

---

# 72. Chart Library

A browser charting library such as Apache ECharts is appropriate.

Only required components should be embedded.

Likely modules:

```text id="8jc6kd"
Line
Bar
Scatter
Boxplot

Grid
Tooltip
Dataset
DataZoom
MarkLine
MarkArea
Legend
```

All assets should ship inside the binary.

No CDN.

---

# 73. Web Application Architecture

Cockpit should deliver:

> **SPA-level user experience without conventional SPA complexity**

Recommended architecture:

```text id="ycbipv"
Rust / Axum
       │
       ├── HTML pages
       ├── HTML fragments
       ├── JSON API
       └── SSE
              │
              ▼
Browser
       │
       ├── HTMX
       ├── lightweight JS/TypeScript
       ├── ECharts
       ├── artifact viewers
       └── graph visualization
```

---

# 74. Why Not a Traditional SPA?

A full client SPA would introduce:

```text id="nfgv8f"
client-side router
client-side state model
large JavaScript bundle
duplicated API state
more build complexity
more synchronization concerns
```

Cockpit already has a strong server-side state model.

Most pages can be rendered efficiently from Rust.

---

# 75. SPA-Like Behavior

The user should still experience:

```text id="p61d2j"
fast navigation
persistent sidebar
persistent top bar
no distracting full-page flashes
browser back/forward support
instant filters
inline table updates
interactive charts
command palette
live status updates
```

---

# 76. Frontend Responsibilities

Rust/server should own:

```text id="iqr7di"
routing
data model
state computation
filter semantics
HTML composition
authorization if later added
artifact metadata
```

Browser JavaScript should own:

```text id="rjfh5x"
chart interaction
command palette
keyboard navigation
image zoom/pan
visual comparison
graph interactions
small ephemeral UI state
```

---

# 77. Live Updates

Use SSE for server → browser updates.

Example events:

```text id="r2k8w5"
project.scan.started
project.scan.completed

feature.changed
feature.health.changed

test.run.completed
benchmark.run.completed
visual.run.completed

attention.created
attention.resolved
```

---

# 78. Live Update UX

Avoid moving content unexpectedly.

Prefer:

```text id="tfpaw2"
subtle highlight
"updated just now"
badge count changes
small status transition
```

Example:

```text id="fedzrx"
Performance

⚠ Regression
↓
✓ Healthy

Updated just now
```

---

# 79. Homepage

The homepage should be the strongest screen in the application.

It should answer:

```text id="n0nt1d"
Is this okay?
What matters?
What changed?
Which feature?
Can I trust the evidence?
```

---

# 80. Homepage Layout

Possible desktop structure:

```text id="etbtz7"
┌──────────────────────────────────────────────────────────────────────┐
│ Invoice App                                     ⌘K Search           │
│ main · a41f92c · clean · updated 34 sec ago                         │
├──────────────┬───────────────────────────────────────────────────────┤
│              │                                                       │
│ Overview     │ HEALTHY WITH 3 ITEMS NEEDING ATTENTION                │
│ Attention 3  │                                                       │
│ Features     │ 37 Healthy · 2 Attention · 1 Broken · 2 Unknown       │
│ Changes      │                                                       │
│              │ Features 42/44 | Tests 983/986 | Perf 11/12          │
│ Tests        │                                                       │
│ Performance  ├───────────────────────────────────────────────────────┤
│ Visual       │ NEEDS ATTENTION                                       │
│              │                                                       │
│ ADRs         │ 🔴 PDF Export                                         │
│ Architecture │    E2E regression                                     │
│ Graph        │                                                       │
│              │ 🟠 Search                                             │
│              │    p95 latency +18%                                   │
│              │                                                       │
│              │ 🟡 Permissions                                        │
│              │    visual evidence stale                              │
│              ├───────────────────────────────────────────────────────┤
│              │ RECENT CHANGES                                        │
│              ├───────────────────────────────────────────────────────┤
│              │ FEATURE MATRIX                                        │
│              ├───────────────────────────────────────────────────────┤
│              │ PERFORMANCE CHANGES                                   │
└──────────────┴───────────────────────────────────────────────────────┘
```

---

# 81. Project Summary Header

The project header should show:

```text id="vu1syb"
Project name
overall state
branch
HEAD
working-tree status
last scan
comparison baseline if active
```

Example:

```text id="2jz3hl"
Invoice App

Healthy with 3 items needing attention

main · a41f92c · clean
Updated 34 seconds ago
```

---

# 82. Summary Metrics

Use compact information.

Good:

```text id="ixk4ys"
Features      37 healthy   2 attention   1 broken
Tests         983 pass     1 fail        2 flaky
Performance   11 healthy   1 regression
Visual        86 current   1 stale
```

Avoid giant KPI cards.

---

# 83. Needs Attention Panel

This is the most important component.

Each item should show:

```text id="5t1zp2"
status
feature
problem
why it matters
when detected
relevant commit
```

Example:

```text id="w4i7aw"
🔴 PDF Export                                 FEAT-032

E2E regression

1 failing E2E test
introduced after 02ac431
7 hours ago

Unit ✓   E2E ✕   Visual ✓   Perf ✓
```

---

# 84. Recent Changes Panel

Example:

```text id="rzetu6"
4h ago
a41f92c
Replace search index

FEAT-019 Search

⚠ performance regression
✓ tests current
✓ visuals current
```

This is more useful than a normal Git log.

---

# 85. Feature Matrix

The feature matrix is the main system-state visualization.

Example:

```text id="auunau"
FEATURE          STATE      UNIT    E2E    VISUAL    PERF    ADR   CHANGED

Authentication  Healthy      ✓       ✓       ✓        ✓      3      2d
Search          Attention    ✓       ✓       ✓        ⚠      2      4h
Dashboard       Healthy      ✓       ✓       ✓        ✓      1      7d
PDF Export      Broken       ✓       ✕       ✓        ✓      2      1h
Permissions     Attention    ✓       ✓       ?        ✓      1      3d
Billing         Healthy      ✓       ✓       ✓        ✓      4     21d
```

---

# 86. Matrix Columns

Possible columns:

```text id="9xpaan"
Feature
Overall State
Unit
Integration
E2E
Visual
Performance
Coverage
ADRs
Freshness
Changed
Importance
```

Users should be able to customize visible columns later.

---

# 87. Matrix Filtering

Top-level filters:

```text id="tpo82s"
All
Broken
Attention
Unknown
Healthy
```

Additional:

```text id="ni4am6"
importance
owner
tag
area
changed since
freshness
evidence type
```

Filtering must be fast.

---

# 88. Matrix Sorting

Useful sort options:

```text id="kfxmms"
importance
severity
recent change
feature name
staleness
performance regression
number of gaps
```

Default:

> importance-adjusted attention severity

---

# 89. Matrix Sparklines

Optional compact performance or reliability history.

Example:

```text id="itx3du"
Search

Perf
▁▁▂▂▃▅▆  +18%

Tests
✓✓✓✓✓✕✓
```

Sparklines should never reduce readability.

---

# 90. Feature Detail Page

The feature detail page should act as a self-contained engineering briefing.

Primary sections:

```text id="vr9drw"
Feature identity
Current state
Reasons
Description
Evidence
Freshness
Performance
Recent change
Architecture
Tests
Visual artifacts
History
```

---

# 91. Feature Header

Example:

```text id="cfjzkq"
Search                                      FEAT-019

⚠ Needs Attention

Search API p95 increased 17.9% after the latest implementation change.

Importance
High

Owner
Search

Changed
4 hours ago
```

---

# 92. Feature Evidence Panel

Example:

```text id="8lkqnz"
Evidence

Unit            ✓ 38 passing
Integration     ✓ 6 passing
E2E             ✓ 4 passing
Visual          ✓ current
Performance     ⚠ regression
Coverage        ✓ 92%
Documentation   ✓ current
```

---

# 93. Feature Freshness Panel

```text id="fxnohl"
Evidence freshness

Latest implementation change:
a41f92c · 4 hours ago

Unit            ✓ after change
Integration     ✓ after change
E2E             ✓ after change
Visual          ✓ after change
Performance     ✓ after change
```

Or:

```text id="nzpzkw"
Visual
⚠ last validated before implementation changed
```

---

# 94. Feature Timeline

Example:

```text id="ply1su"
09:12   a41f92c  implementation changed
09:15             unit tests passed
09:17             integration tests passed
09:20             E2E passed
09:21             visual tests passed
09:25             performance regression detected
```

This gives powerful temporal context.

---

# 95. Tests Page

Project-wide test view:

```text id="0zo021"
Tests

986 total

983 passing
1 failing
2 flaky

[ Failing ] [ Flaky ] [ Slow ] [ Recent ] [ All ]
```

Test table:

```text id="dhknd3"
STATUS  TEST                         FEATURE     KIND     DURATION  LAST RUN

✕       PDF export retains layout    FEAT-032    E2E      3.84s     7h
⚠       Search handles typo          FEAT-019    E2E      1.91s     2h
✓       Token cannot be reused       FEAT-012    Unit     8ms       1h
```

---

# 96. Test Detail Page

Should include:

```text id="rcocew"
name
status
feature
kind
source
duration
history
flakiness
commit
artifacts
failure output
```

---

# 97. Performance Page

Top:

```text id="gkmblp"
Performance

1 budget breach
2 regressions
34 healthy
3 incomparable
```

Primary sections:

```text id="a07o4t"
Regressions
Budget breaches
Largest improvements
Benchmark matrix
Environment selector
```

---

# 98. Benchmark Table

Example:

```text id="qyurw7"
BENCHMARK               VALUE        CHANGE       BUDGET       STATE

Search API p95          132 ms       +17.9%       150 ms       ⚠
PDF render              1.82 s        +7.4%         2 s        ⚠
Auth middleware         0.42 ms       +1.1%         1 ms       ✓
Serialization             81 µs       -6.2%       100 µs       ✓
```

---

# 99. Benchmark Detail Chart

Should display:

```text id="0v2thn"
measurement history
budget threshold
baseline
release markers
commit markers
current measurement
```

Optional advanced overlays:

```text id="ap0med"
confidence interval
rolling median
regression region
```

---

# 100. Benchmark Distribution View

For noisy benchmarks:

```text id="a05juy"
Baseline distribution
Current distribution
confidence intervals
outliers
```

This should be an advanced view rather than the default.

---

# 101. Visual Page

Visual testing overview:

```text id="ydhr9u"
Visual Validation

86 current
2 stale
1 regression
4 missing
```

Group by:

```text id="jz8rsd"
feature
page
browser
viewport
status
```

---

# 102. Changes Page

This page should answer:

> What changed, and did validation catch up?

Example:

```text id="lh7kv2"
a41f92c
Replace search index
4h ago

Affected
Search

Validation after change

Unit           ✓
Integration    ✓
E2E            ✓
Visual         ✓
Performance    ⚠ regression
```

---

# 103. ADR Page

ADRs should preserve the reading quality of Markdown.

Header:

```text id="23ue94"
ADR-022

Use inverted index for search

Accepted
Aug 19, 2026

Affected
Search
Suggestions
```

Content:

```text id="dmnsck"
Context
Decision
Alternatives
Consequences
```

Related engineering state should appear alongside or below the document.

---

# 104. Architecture Documentation

Architecture documents should remain long-form Markdown.

Useful sidebar:

```text id="t6iwoq"
Related features
Related ADRs
Recent commits
```

Cockpit should not force every document into feature-style cards.

---

# 105. Graph View

The graph view represents relationships.

Possible nodes:

```text id="ypqem1"
feature
ADR
test
benchmark
commit
artifact
PR
issue
```

Default graph should probably start feature-centered rather than showing the entire repository.

---

# 106. Graph UX

Interactions:

```text id="z127hx"
click node
focus neighbors
hide node types
search
zoom
pan
expand depth
open detail
```

Example:

```text id="x76w1x"
             ADR-022
                │
                ▼
             Search
          ┌─────┼─────┐
          ▼     ▼     ▼
        E2E   PERF   VIS
                ▲
                │
             commit
```

---

# 107. Global Search

Global search should include:

```text id="wqaxzy"
features
ADRs
tests
benchmarks
commits
documents
artifacts
```

Example:

```text id="a4k9zs"
> password

Features
Password Reset                    FEAT-012

ADRs
Reset Token Storage               ADR-007

Tests
Reset token expires after 30m

Commits
81fc913 Rotate refresh tokens
```

---

# 108. Command Palette

Open:

```text id="ikj5mc"
Cmd/Ctrl + K
```

Possible commands:

```text id="v6pwmf"
Go to Overview
Go to Feature
Show Broken Features
Show Stale Evidence
Show Benchmark Regressions
Show Recent Changes
Refresh Project
Toggle Theme
Copy Current ID
Open Repository File
```

---

# 109. Keyboard Navigation

Suggested shortcuts:

```text id="9fzd9r"
/        search

Cmd+K    command palette

g o      overview
g a      attention
g f      features
g c      changes
g t      tests
g p      performance
g v      visual

j        next
k        previous
Enter    open
Esc      back/close
```

Keyboard support should feel native rather than added later.

---

# 110. Visual Design Direction

Cockpit should feel:

```text id="zs0gy0"
calm
technical
premium
precise
dense
fast
trustworthy
```

Conceptual inspiration:

```text id="eozkq4"
Linear
Raycast
GitHub
Datadog
IDE tooling
high-end engineering instruments
```

Avoid direct imitation.

---

# 111. Dark Theme

Primary design target.

Conceptual palette:

```text id="ieew8j"
Background             deep graphite
Panel                  slightly elevated graphite
Border                 subtle gray
Primary text           near-white
Secondary text         cool gray
Muted text             lower-contrast gray
```

Status:

```text id="dx56zu"
Healthy     green
Attention   amber
Broken      red
Unknown     muted blue/gray
Change      blue
```

---

# 112. Light Theme

Light mode should maintain:

```text id="130po4"
strong hierarchy
restrained borders
dense layout
high contrast
subtle status tinting
```

It should not merely invert the dark palette.

---

# 113. Typography

UI font:

```text id="sv0yc2"
clean modern sans-serif
```

Monospace font:

```text id="mpnq5h"
technical IDs
commits
paths
measurements
code
```

Example:

```text id="4oyfju"
Search
FEAT-019

132 ms
a41f92c
src/search/index.rs
```

---

# 114. Density

Project Cockpit should make good use of large displays.

Avoid:

```text id="f4s5za"
huge cards
large empty areas
single metrics in entire rows
```

Prefer:

```text id="obnc88"
dense tables
compact rows
small status summaries
well-grouped panels
```

---

# 115. Card Philosophy

Cards should exist only where they establish meaningful visual grouping.

Avoid nested cards inside cards.

Much of Cockpit may be better expressed through:

```text id="o67hdn"
sections
rows
tables
dividers
panels
```

rather than dashboard tiles.

---

# 116. Status Language

Always combine:

```text id="gfzkls"
icon
label
color
```

Examples:

```text id="pc8xhz"
✓ Healthy
⚠ Needs Attention
✕ Broken
? Unknown
↻ Stale
```

Do not rely on color alone.

---

# 117. Loading UX

Cockpit should normally load quickly.

When loading is necessary:

```text id="xjxbd7"
skeleton rows
localized spinners
progress text
```

Avoid giant blocking loading screens.

---

# 118. Scanning UX

During initial scan:

```text id="29bogx"
Scanning project...

Features        42
ADRs            23
Tests         1,291
Benchmarks      34
Git commits    412
```

This could stream incrementally.

---

# 119. Empty States

Useful empty state:

```text id="whg39v"
No performance evidence

No benchmark output was found for this feature.

Configured sources:
artifacts/performance/**
```

Bad empty state:

```text id="dfo99n"
Nothing here.
```

---

# 120. Error States

Errors should be specific.

Example:

```text id="zmcqgd"
Could not parse Playwright results

artifacts/playwright/results.json

Unexpected value at line 182.

[ View parser details ]
```

---

# 121. Partial Failure

A malformed benchmark file should not prevent the rest of Cockpit from loading.

Prefer:

```text id="p1a1z4"
Project loaded with 1 import warning.
```

Cockpit should be resilient to imperfect repositories.

---

# 122. Importer Architecture

All external evidence should enter through adapters.

```rust id="k88t2s"
trait Importer {
    fn kind(&self) -> ImporterKind;

    fn detect(&self, path: &Path) -> DetectionResult;

    fn import(
        &self,
        context: &ImportContext,
        path: &Path,
    ) -> Result<Vec<ImportedEntity>>;
}
```

---

# 123. Initial Importers

Potential built-ins:

```text id="srhqn4"
Markdown Feature
Markdown ADR
JUnit
Playwright
LCOV
Cobertura
Criterion
Iai-Callgrind
cargo-nextest
Cockpit JSON
```

---

# 124. Interchange Format

Instead of dynamic plugins initially, define stable JSON formats.

Example:

```json id="waw9y8"
{
  "schema": "cockpit.test.v1",
  "id": "TEST-992",
  "feature_ids": ["FEAT-012"],
  "status": "passed",
  "duration_ms": 391,
  "commit": "a41f92c",
  "generated_at": "2026-09-11T08:42:13Z",
  "artifacts": [
    {
      "type": "video",
      "path": "reset-password.webm"
    }
  ]
}
```

This provides interoperability across languages.

---

# 125. Cockpit Benchmark Format

Example:

```json id="xuw8ru"
{
  "schema": "cockpit.benchmark.v1",
  "id": "PERF-019",
  "feature_ids": ["FEAT-019"],
  "metric": {
    "name": "search.p95",
    "unit": "ms",
    "direction": "lower_is_better"
  },
  "budget": {
    "max": 150
  },
  "run": {
    "commit": "a41f92c",
    "environment": "ci-linux-x64",
    "value": 132
  }
}
```

---

# 126. Derived SQLite Index

Recommended:

```text id="f9f6nu"
.cockpit/index.sqlite
```

Possible tables:

```text id="8iu0zu"
features
adrs
tests
test_runs
benchmarks
benchmark_runs
measurements
visual_evidence
artifacts
commits
relationships
attention_items
scan_files
environments
```

---

# 127. SQLite Role

SQLite is:

```text id="pcl4gg"
cache
index
query accelerator
history store where appropriate
```

SQLite is not:

```text id="58r2f5"
project source of truth
```

Deleting it should be safe.

---

# 128. Incremental Scanning

Cockpit should avoid rescanning everything.

Track:

```text id="tgmh36"
file path
mtime
size
content hash where needed
importer version
schema version
```

Only affected entities should be invalidated.

---

# 129. Dependency Recalculation

If:

```text id="vz4tye"
FEAT-019.md changes
```

Cockpit may need to recompute:

```text id="7ceujx"
feature metadata
relationships
health
attention
search index
```

If:

```text id="4axalj"
screenshot.png changes
```

only visual evidence may need updating.

---

# 130. Filesystem Watching

Use native file watching where possible.

Debounce bursts.

Agent activity may modify dozens of files rapidly.

Cockpit should avoid recomputing after every individual filesystem event.

Possible behavior:

```text id="kh001w"
event burst
↓
250ms debounce
↓
collect changed paths
↓
incremental rescan
↓
recompute affected state
```

---

# 131. Server Architecture

Possible request pipeline:

```text id="4eu0v0"
Browser request
      ↓
Axum route
      ↓
Application service
      ↓
Project state / query layer
      ↓
MiniJinja template
      ↓
HTML
```

JSON endpoints reuse the same application service.

---

# 132. Route Structure

Possible routes:

```text id="vulfa2"
/                         Overview

/attention
/features
/features/:id

/changes
/commits/:sha

/tests
/tests/:id

/performance
/performance/:id

/visual
/visual/:id

/adrs
/adrs/:id

/architecture/*
/graph
```

---

# 133. API Structure

```text id="91je0t"
GET /api/v1/project
GET /api/v1/status
GET /api/v1/attention

GET /api/v1/features
GET /api/v1/features/:id

GET /api/v1/tests
GET /api/v1/tests/:id

GET /api/v1/benchmarks
GET /api/v1/benchmarks/:id

GET /api/v1/changes
GET /api/v1/commits/:sha

GET /api/v1/graph
```

---

# 134. Internal Application Services

Useful service boundaries:

```text id="ee8vu5"
ProjectService
FeatureService
AttentionService
EvidenceService
GitService
BenchmarkService
TestService
ArtifactService
SearchService
ValidationService
```

These should be internal abstractions rather than separate processes.

---

# 135. Search Architecture

Initial search can search structured entities.

Later:

```text id="yoh7p8"
full Markdown body
test names
commit messages
paths
ADR content
```

A Rust search engine such as Tantivy may eventually be useful.

---

# 136. Query Language

A lightweight structured query language could become extremely valuable.

Examples:

```text id="5ntqln"
state = broken

importance = critical

performance.regression = true

visual = stale

changed_since = "7d"

adr = "ADR-022"
```

CLI:

```bash id="5zv5ac"
cockpit query 'state = broken and importance = critical'
```

---

# 137. Validation Engine

Cockpit should support project policies.

Examples:

```text id="xfsv19"
all feature references resolve
all accepted ADR references resolve
critical features require E2E evidence
stable user-facing features require visual evidence
performance-critical features require benchmarks
no critical feature may remain unknown
```

---

# 138. Validation Output

Example:

```text id="6dr1ww"
Project: Invoice App

✓ 48 feature definitions valid
✓ 23 ADRs valid
✓ 1,291 tests imported
✓ All references resolve

✕ FEAT-019 missing required E2E coverage
✕ FEAT-032 references ADR-042, which does not exist

⚠ FEAT-014 visual evidence predates latest implementation change
⚠ PERF-019 has no comparable baseline
```

---

# 139. Validation Configuration

Example:

```toml id="bkblmt"
[policy.critical_features]
require_e2e = true
require_visual = true
require_fresh_evidence = true

[policy.performance]
require_comparable_environment = true

[policy.references]
fail_on_broken_reference = true
```

---

# 140. CI Integration

Example:

```bash id="wd31me"
cockpit validate --strict
```

CI can fail if project state violates configured policies.

This converts engineering documentation conventions into enforceable constraints.

---

# 141. Agent Workflow

An agent implementing a feature could:

```text id="fxk6wk"
1. create FEAT-044
2. implement code
3. create ADR if required
4. add tests
5. add Playwright flow
6. generate screenshots
7. run benchmarks
8. commit with feature reference
```

Cockpit evaluates independently.

---

# 142. Agent Feedback Loop

Agent can call:

```bash id="jk098c"
cockpit feature FEAT-044 --json
```

and receive:

```json id="kfup4i"
{
  "state": "attention",
  "reasons": [
    {
      "type": "missing_visual_evidence"
    }
  ]
}
```

The agent can then remediate the gap.

This turns Cockpit into a machine-readable definition of "done."

---

# 143. Acceptance Criteria

A later version may give acceptance criteria stable identities.

Example:

```text id="yf2b1r"
FEAT-012.AC-1
FEAT-012.AC-2
```

Tests can reference them.

Then Cockpit can show:

```text id="nos4xv"
Acceptance criteria

3 total
3 covered
3 passing
```

---

# 144. Release Comparison

Cockpit should support:

```bash id="znzomg"
cockpit serve --baseline v1.12.0
```

or an in-UI baseline selector.

Comparison:

```text id="osn8dd"
Since v1.12.0

4 features added
2 features changed
84 tests added
2 ADRs accepted

Performance
1 regression

Visual
3 updated baselines

Health
0 broken → 1 broken
```

---

# 145. Historical Snapshots

A future format could store snapshots:

```text id="s5teoq"
.cockpit/snapshots/
```

or release artifacts.

This allows comparing engineering state across time even when raw artifacts are no longer locally available.

---

# 146. "Since You Last Looked"

Optional local user state:

```text id="tidocj"
Last viewed commit:
91b72ad
```

Cockpit could then show:

```text id="jxikwi"
Since your last visit

12 commits
3 features changed
1 new feature

2 new attention items
1 resolved issue
```

This should be optional and local.

---

# 147. Multi-Project Future

Each project could publish:

```text id="zqrowe"
project-state.json
```

Workspace mode:

```bash id="xtq9hy"
cockpit workspace serve
```

Overview:

```text id="zswzpf"
Invoice App          ✓ Healthy
Search Service       ⚠ 2 attention items
Admin Portal         ✕ Broken
Prototype            ? incomplete evidence
```

---

# 148. Multi-Project Principle

Workspace mode should aggregate summaries.

Deep drill-down should still happen inside the project model.

Avoid creating another enterprise portal layer too early.

---

# 149. Security Defaults

Default binding:

```text id="u6dxmp"
127.0.0.1
```

Remote binding requires explicit configuration.

---

# 150. Artifact Path Security

Artifact serving must:

```text id="nu41hy"
canonicalize paths
prevent ../ traversal
restrict roots
avoid exposing arbitrary repository secrets
```

---

# 151. Markdown Security

If raw HTML is supported in Markdown, sanitize it or require explicit trust.

Default should prioritize safe local rendering.

---

# 152. Credential Handling

GitHub or other optional integration tokens must:

```text id="9d6zrb"
never appear in HTML
never appear in JSON responses
never appear in logs by default
```

---

# 153. Performance Expectations for Cockpit

Reasonable goals:

```text id="6brfyo"
warm startup             near-instant for normal projects
initial overview         < 1 second where practical
local navigation         perceptually instant
incremental scan         affected files only
large images             lazy loaded
video                    streamed
```

---

# 154. Large Repository Behavior

Cockpit should degrade gracefully.

Possible strategies:

```text id="tff9li"
incremental Git scanning
pagination
lazy historical loading
limited default benchmark history
background-free on-demand computation
database indexes
```

The overview should remain fast even when deep history is large.

---

# 155. Caching

Potential cache layers:

```text id="38z4yz"
parsed Markdown AST
normalized entities
Git metadata
search index
render-independent state
benchmark comparison results
```

Avoid caching HTML excessively unless profiling justifies it.

---

# 156. Observability of Cockpit Itself

Optional diagnostic command:

```bash id="6v4hmv"
cockpit doctor
```

Could report:

```text id="il7scf"
project root detected
Git available
42 feature files
23 ADR files
Playwright report found
Criterion results found
3 importer warnings
index healthy
```

---

# 157. Logging

Default logs should be quiet.

Possible levels:

```text id="bmsmbm"
error
warn
info
debug
trace
```

CLI:

```bash id="slysvb"
cockpit serve --log debug
```

---

# 158. Rust Module Layout

Potential workspace:

```text id="3brzrs"
cockpit/
├── crates/
│   ├── cockpit-cli/
│   ├── cockpit-core/
│   ├── cockpit-model/
│   ├── cockpit-server/
│   ├── cockpit-ui/
│   ├── cockpit-git/
│   ├── cockpit-import/
│   ├── cockpit-benchmark/
│   ├── cockpit-validation/
│   └── cockpit-storage/
```

For v0.1, fewer crates may be preferable.

Avoid architecture astronautics.

---

# 159. Suggested Rust Ecosystem

Possible choices:

```text id="eo414k"
Axum
Tokio
Serde
serde_json
toml
Comrak
MiniJinja
rusqlite
rust-embed
notify
git2 or process-based Git
```

Optional later:

```text id="9o39jv"
Tantivy
```

---

# 160. Frontend Build Strategy

Cockpit itself may use a small frontend build step during development.

For example:

```text id="ym2580"
TypeScript
CSS processing
ECharts tree-shaking
asset minification
```

Release build then embeds compiled assets into Rust.

Users never need Node.

---

# 161. UI Asset Strategy

Embedded assets:

```text id="j50h4k"
app.css
app.js
htmx.js
echarts.js
icons.svg
templates
```

Rust serves:

```text id="u9dcnq"
/assets/app.css
/assets/app.js
```

from embedded resources.

---

# 162. Design System Tokens

A designer should define tokens rather than isolated styles.

Possible groups:

```text id="szusn2"
background
surface
surface-hover
border
border-strong

text-primary
text-secondary
text-muted

status-healthy
status-attention
status-broken
status-unknown
status-info
```

---

# 163. Spacing

Suggested base scale:

```text id="ra4b6y"
4
8
12
16
24
32
48
```

Dense screens should rely heavily on:

```text id="1iz849"
4
8
12
16
```

---

# 164. Borders and Radius

Cockpit should use relatively modest rounding.

Suggested:

```text id="oe3wzj"
4px
6px
8px
```

Avoid excessive pill-shaped surfaces.

---

# 165. Elevation

Most screens should remain fairly flat.

Use elevation mainly for:

```text id="1nd4hd"
popover
command palette
modal
tooltip
floating artifact viewer
```

---

# 166. Responsive Strategy

Primary:

```text id="vg7q7s"
1440px+
```

Secondary:

```text id="zv3g5v"
1024–1440px
```

Mobile:

functional but not primary.

---

# 167. Responsive Feature Matrix

On narrow screens:

```text id="wq9uqu"
Feature + State remain sticky
secondary columns horizontally scroll
least important columns hide first
```

Never reduce all information to opaque cards if a table remains more useful.

---

# 168. Accessibility

Requirements:

```text id="fr1d2e"
keyboard navigation
semantic table markup
visible focus
screen-reader status text
non-color status indicators
sufficient contrast
reduced motion
usable zoom
```

---

# 169. Motion

Animations should be short and subtle.

Possible:

```text id="w1px5a"
120–180 ms
```

Avoid animation when it delays information comprehension.

---

# 170. Visual Priority

A healthy screen should feel calm.

Cockpit should not paint the interface green.

Healthy state should mostly look neutral.

Problems use stronger visual attention.

This improves signal-to-noise.

---

# 171. Notification Philosophy

Cockpit is primarily an inspection system, not a notification center.

Avoid intrusive toast spam.

Use toasts mainly for user-triggered operations:

```text id="hfulqb"
scan complete
copied ID
baseline changed
```

New engineering state should appear naturally in the screen.

---

# 172. Deep Linking

Every important entity should have a stable local URL.

Examples:

```text id="tb7jyy"
/features/FEAT-019
/adrs/ADR-022
/tests/E2E-122
/performance/PERF-019
/commits/a41f92c
```

This makes agent → human handoff straightforward.

---

# 173. URL Filters

Useful filters should be encoded in URLs where practical.

Example:

```text id="e0ex06"
/features?state=attention&importance=high
```

This enables sharing and browser history.

---

# 174. Breadcrumbs

Example:

```text id="ahjcw7"
Features / Search / Performance / Search API p95
```

Breadcrumbs should remain visually quiet.

---

# 175. Tooltips

Use for compact explanations.

Example:

```text id="9zl47p"
⚠

Performance regression

132 ms
+17.9%

Still inside 150 ms budget.
```

Critical information must not exist only in a tooltip.

---

# 176. Help and Explainability

A user should be able to inspect:

> Why is this feature Attention?

Possible interaction:

```text id="5rvkiy"
Why?
```

Result:

```text id="7u2zdt"
Search is marked Needs Attention because:

1. PERF-019 increased 17.9%.
2. The change exceeds the configured 10% regression threshold.
3. The metric remains below its 150 ms hard budget.
```

This transparency builds trust.

---

# 177. State Confidence

Optional internal model:

```text id="rz218g"
High
Medium
Low
```

Example:

```text id="b7t720"
Performance regression
Confidence: High

Feature affected by commit
Confidence: Medium
```

Do not overexpose confidence unless it helps the user.

---

# 178. Explicit vs Inferred Relationships

Relationships may be:

```text id="b9ax40"
Explicit
Inferred
```

Example:

```text id="qif4av"
a41f92c → FEAT-019

Source:
path match src/search/**
```

This supports debugging Cockpit's own reasoning.

---

# 179. User Overrides

Later, allow users to suppress or annotate attention.

Example:

```text id="nic1kl"
Acknowledge until next change

Reason:
Expected performance tradeoff from new search quality algorithm.
```

However, avoid turning Cockpit into an issue management system.

---

# 180. Design Deliverables

The web designer should deliver:

## Foundation

```text id="deyz97"
color system
typography
spacing
icons
status language
dark theme
light theme
```

## Primary screens

```text id="ziip3r"
Overview
Attention
Feature Matrix
Feature Detail
Changes
Performance
Benchmark Detail
Tests
Visual Regression
ADR
```

## Interaction patterns

```text id="r5thbb"
filters
search
command palette
tooltips
drawers
dialogs
artifact viewer
chart interactions
```

## States

```text id="rhe5ng"
loading
empty
error
partial error
unknown
stale
live update
```

---

# 181. Priority of Design Work

Design in this order:

```text id="1zn94t"
1. Overview
2. Feature Matrix
3. Feature Detail
4. Attention
5. Benchmark Detail
6. Changes
7. Visual Regression
8. Test Detail
9. ADR
10. Graph
```

The first five define most of the product language.

---

# 182. v0.1 Product Scope

## Inputs

```text id="azxcvu"
Feature Markdown
ADR Markdown
Git
JUnit
Playwright
screenshots
videos
traces
Criterion/basic benchmark data
```

## Core state

```text id="c23aij"
feature health
attention
freshness
Git correlation
benchmark regression
visual regression
```

## Views

```text id="5y59d3"
Overview
Attention
Features
Feature Detail
Changes
Tests
Performance
Benchmark Detail
Visual Detail
ADR Detail
```

---

# 183. v0.1 Explicit Non-Goals

Do not require:

```text id="zk99bc"
workspace aggregation
advanced graph visualization
GitHub write actions
issue management
custom plugins
full acceptance-criterion graph
notifications
user accounts
remote hosting
```

---

# 184. v0.2 Possibilities

```text id="518qsm"
coverage integration
test flakiness modeling
advanced benchmark statistics
Iai-Callgrind
baseline selector
release comparison
global search
structured queries
GitHub enrichment
```

---

# 185. v0.3 Possibilities

```text id="0lx4kx"
acceptance criteria mapping
workspace overview
custom policies
historical snapshots
agent MCP integration
advanced graph
release readiness report
```

---

# 186. Long-Term Possibilities

Potentially:

```text id="22xjhd"
multi-repository dependencies
service relationships
production observability evidence
security evidence
dependency vulnerability state
deployment status
release automation
```

Only add these if they preserve Cockpit's core clarity.

---

# 187. Risks

## Risk: Too much information

Mitigation:

```text id="83tspg"
attention-first hierarchy
progressive disclosure
feature-centric organization
```

## Risk: Becoming a generic dashboard

Mitigation:

```text id="5u6ld8"
focus on engineering decisions
state interpretation
change correlation
```

## Risk: False confidence

Mitigation:

```text id="ial2hx"
explicit unknown state
freshness
provenance
comparability rules
```

## Risk: Tool integration fragility

Mitigation:

```text id="9b6lgr"
normalized internal model
importer adapters
Cockpit interchange formats
```

## Risk: Frontend complexity

Mitigation:

```text id="05jjfk"
server-driven architecture
selective JavaScript
embedded frontend assets
```

---

# 188. Product Success Criteria

Cockpit succeeds when the user can quickly answer:

```text id="hs5lfm"
Is this project healthy?

What is broken?

What should I look at first?

What changed recently?

Which feature is affected?

Has it been revalidated after the change?

Is performance worse?

When did the regression begin?

Which commit is related?

What evidence supports this?

Can I inspect the exact artifact?
```

---

# 189. Overview Success Test

A user unfamiliar with the latest project state should be able to open Cockpit and, without clicking anything, state approximately:

```text id="72btqw"
"The app is broadly healthy.

PDF export is broken because an E2E test started failing after today's refactor.

Search is still functional but performance has regressed around 18%.

Permissions changed recently and still needs visual revalidation."
```

If the homepage cannot communicate that, it is not doing its job.

---

# 190. Feature Detail Success Test

The user should be able to open one feature and answer:

```text id="6u2fwh"
What is it?

What changed?

What currently fails?

What evidence passed?

What evidence is stale?

What benchmark changed?

Which ADR explains it?

Which commit likely matters?
```

without navigating through five separate tools.

---

# 191. Benchmark Success Test

A benchmark view should answer:

```text id="d0bhnp"
What is the current value?

Is it better or worse?

Compared to what?

Is the change statistically meaningful?

Is the budget violated?

Are environments comparable?

When did the shift happen?

Which commit aligns with the change?
```

---

# 192. Trust Success Test

For any Cockpit claim, the user should be able to ask:

> Why?

and get a concrete chain such as:

```text id="l5by2t"
FEAT-019 Needs Attention

because

PERF-019 regressed 17.9%

because

current comparable CI run = 132 ms
baseline comparable CI run = 112 ms

and

configured regression threshold = 10%

but

budget = 150 ms

therefore

Needs Attention, not Broken
```

That is the level of transparency Cockpit should aim for.

---

# 193. Final Product Vision

Project Cockpit should become a **local engineering intelligence layer** for software repositories.

Not a static documentation viewer.

Not a dashboard made of counters.

Not another test report.

It should continuously turn:

```text id="v1tpd3"
intent
+
implementation
+
tests
+
performance
+
visual evidence
+
architecture
+
Git history
```

into:

```text id="jqb8jg"
STATE
```

Then turn state into:

```text id="1i8bw0"
ATTENTION
```

Then connect attention to:

```text id="hjxbnq"
CHANGE
```

Then connect change to:

```text id="ny5oew"
FEATURES
```

And connect every conclusion back to:

```text id="flfxtk"
EVIDENCE
```

The finished product should feel like opening the instrument panel of the software project and immediately seeing:

> **This is where the system stands.**

> **These are the things that matter.**

> **This is what changed.**

> **This is what we know.**

> **This is what we do not know.**

> **This is why.**

That should remain the governing principle for every future product, architecture, UX, visualization, and implementation decision.