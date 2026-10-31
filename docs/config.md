# cockpit.toml

Discovery: `--project <path>` → else walk up from the current directory for `cockpit.toml` or
`cockpit/cockpit.toml` (the first found wins, even past a `.git`) → else the nearest `.git`
is an implicit project with conventional sources. `.cockpit/` (index, pulled evidence) lives
beside the config file and ignores itself.

Every table rejects unknown keys.

```toml
[project]
id = "paged"                 # slug; default: the root directory name
name = "Paged"
root = ".."                  # project root relative to this file (umbrella projects)
primary_branch = "main"

[[repos]]                    # optional; default: the root if it has .git, else every dir with .git (depth ≤ 2)
id = "core"
path = "core"
primary_branch = "main"

[features]
path = "docs/features"       # recursive; subdirectories are groups; _chapter.md names a group
id_pattern = "^FEAT-\\d{3}$" # optional; enables bare-id matching in commits and test names
default_importance = "normal"
default_lifecycle = "stable"

[adrs]
path = "docs/adr"            # string or list
exclude = ["0000-template.md", "README.md"]

[[components]]               # optional vocabulary for claims:
id = "core.parser"
title = "Parser"
group = "core"

[git]
implementation_paths_exclude = ["docs/**", "**/*.md"]   # commits touching only these never make evidence stale
max_commits = 5000
dirty_check = true
skip_repos = ["state"]

[[evidence]]
id = "core/nextest-unit"     # <source>/<suite>; a configured source is one suite
title = "core cargo-nextest"
format = "junit"             # junit | playwright | cockpit-results | paged-results | cockpit-benchmark
                             # | guardian-bench | paged-measurements | criterion | fidelity | cockpit-visual | auto
kind = "unit"                # unit | integration | e2e | visual | benchmark | conformance
path = [".cockpit/evidence/core__nextest-unit/**", "core/target/nextest/ci/junit.xml"]
component = "core.parser"
repo = "core"                # commit resolution and freshness happen in this repository
features = ["FEAT-024"]      # default feature links for everything in the source
expect_every = "48h"         # silence threshold (default: health.suite_silence for github-backed sources)
allow_outside_root = false   # absolute path globs are only followed with true
[evidence.github]            # cockpit pull
repo = "paged-media/core"
workflow = "ci.yml"
artifact = "nextest-junit"
branch = "main"
runs_to_inspect = 20
[[evidence.split]]           # route Cargo packages of a workspace report to another source
packages = ["paged-script"]
id = "core-script/nextest-canvas-wasm"
component = "core.canvas-wasm"
[evidence.validity]
treat_all_failed_as_invalid = true
all_failed_min_tests = 20

[test_map]
files = ["test-map.yaml"]    # YAML/TOML entries: match{source,suite,file,title,classname,ac_prefix}, features, component, level

[health]
max_evidence_age = "7d"
flaky_window = 20
flaky_failures = 3
broken_on_failure_kinds = ["unit", "integration", "e2e"]
required_evidence = { critical = ["test"], high = ["test"], normal = [], low = [] }   # "test" = unit | integration | e2e
suite_silence = "48h"
duration_regression_pct = 30
duration_regression_min_ms = 500

[performance]
baseline = "previous-comparable"   # | "tag:<name>" | "commit:<sha>"
history_limit = 200
[performance.policy]
deterministic = { noise_pct = 0, gate = true }
semi_deterministic = { noise_pct = 10, gate = true }
environment_sensitive = { noise_pct = 25, gate = false, require_significance = false }
[[performance.metrics]]      # defaults for metrics a producer did not classify
match = "*/*/commit.calls"   # glob over <name>/<subject>/<metric>
determinism = "deterministic"
direction = "lower_is_better"
budget = { max = 12 }

[policy.references]
fail_on_broken_reference = true
[policy.performance]
require_comparable_baseline = false   # PERF001
[policy.claims]
drift = "ignore"             # ignore | warn | error   (CLAIM001)
[policy.checks]              # per-check overrides: EV003 = "ignore" | "warn" | "error"
EV003 = "warn"

[scan]
max_file_size = "64MiB"

[server]
bind = "127.0.0.1"
port = 4747
artifact_roots = []          # absolute directories that /artifacts/external may serve

[pull]
dir = ".cockpit/evidence"
max_artifact_bytes = 536870912
timeout_secs = 120

[commits]
id_patterns = ["ADR \\d{3}"] # extra informational ids recognised in commit messages
```

Conventional sources (no `[[evidence]]`): `**/target/nextest/*/junit.xml`, `artifacts/tests/**/*.xml`,
`**/test-results/junit.xml`, `**/test-results/results.json`, `.cockpit/evidence/**/*.json`
(sniffed), `perf/results/*.json` (Guardian bench), `**/paged-measurements-*.json`,
`**/paged-results*.json`, `**/target/criterion/**/new/estimates.json`, `**/gate.json`.

## Agentic development (experimental)

The optional `[development]` section configures the same Rust feedback policy for
local Claude Code agents and GitHub CI. It does not alter feature health or run
commands during `cockpit scan`.

```toml
[development]
rust_edit_backend = "none"          # none | cargo | blaze (default: none)
rust_ci_backend = "cargo"           # cargo | blaze_then_cargo (default: cargo)
downstream = "none"                 # none | direct | deep: add Moon dependents (default: none)
# Repository-relative globs. A match inside a project plans that project's tests (test_data)
# or its checks and tests (generated); outside every project the file stays unknown.
test_data = ["**/fixtures/**", "**/__fixtures__/**", "**/testdata/**",
             "**/__snapshots__/**", "**/proptest-regressions/**"]   # the default
generated = []                      # e.g. ["apps/editor/apps/canvas/feature-ids.json"]
documentation = []                  # non-Markdown docs: no checks, like *.md
# Inputs other projects' tests read, as glob = [Moon project ids]: plans those projects' tests.
test_inputs = { "docs/development/surface-parity-matrix.json" = ["editor"] }
```

Setting `test_data` replaces the default list. A file that is neither source (by name:
`*.rs`, `*.ts`…, `Cargo.toml`, `Cargo.lock`, `package.json`, `tsconfig*.json`) nor
documentation nor matched by one of these classes keeps the plan's scope `unknown`.
A repository without its own `cockpit.toml` (a worktree planned from an umbrella
registry, say) can take this section from another file: `cockpit plan --policy FILE`.

An opt-in Blaze edit backend provides fast, advisory feedback. In CI,
`blaze_then_cargo` adds an advisory preflight, followed by the repository's
pinned Cargo build/check and tests. Blaze never replaces Cargo as the
compatibility gate. See [agentic SDLC](agentic-sdlc.md) for the staged
Rust/TypeScript/Playwright workflow, explicit `cockpit verify` execution,
Claude Code usage metrics and limitations of path-based test selection.
