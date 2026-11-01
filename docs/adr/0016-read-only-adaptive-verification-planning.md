# ADR-0016 — Read-only adaptive verification planning

**Status:** Proposed (2026-10-31)
**Date:** 2026-10-31

## Context

Agentic development cycles waste time and tokens by repeatedly exploring source and invoking
broad builds or test suites after small edits. Cockpit already derives feature health from
tests, Git history, and other evidence. It must not treat a proposed check as proof.

## Decision

Introduce an optional, read-only development-planning layer in Cockpit. Its first interface is
`cockpit plan`, which identifies changed paths, maps source files to Moon project roots
when a map exists, and returns staged local verification suggestions. The command neither
executes the suggestions nor mutates evidence or feature health.

- **Edit:** propose cheap feedback only when explicitly requested (Blaze is advisory).
- **Checkpoint:** propose project-scoped checks and tests; never assert transitive completeness.
- **CI:** defer to the repository's existing authoritative CI workflows.

Unknown or configuration-wide changes must remain explicitly unscoped; do not silently choose
an incomplete subset of checks. Results expose input provenance, uncertainty, execution status
(`not_run`), and a versioned JSON envelope (`cockpit.plan.v1`).

The initial implementation is path-based. It is not a language-server-backed symbol graph,
test coverage analyzer, build engine, or verification evidence source.

## Boundaries and follow-ups

The feature is opt-in and independent of Cockpit's evidence index and UI scans. Future
work may add precise language-server relationships, test-to-code mappings, explicit
verification execution, snapshot-addressed result caching, and resource-aware scheduling.
Each requires separate validation before it can influence health or gate decisions.

Blaze, Cargo, Moon and Bun remain separate tools. Blaze's own Rust toolchain is not
authoritative for projects pinned to another Rust toolchain. Existing CI stays the final gate.

## Consequences

A coding agent can request a short, predictable plan without running tests. No false
success or health status is introduced by a plan, but project mapping alone does not
guarantee complete verification coverage.

## Explicit execution extension

An opt-in `cockpit verify` CLI is permitted to run the checked-in plan's commands.
The planning path remains side-effect-free. Verification writes local logs, returns
per-command status, fails closed for incomplete scope and does not change Cockpit
feature health. A CI-mode result only covers the selected commands; specialized
repository CI workflows remain authoritative. The Rust CI mode always includes
pinned Cargo (Blaze, if enabled, is only an advisory preflight).

### Test selection and explicit execution

An opt-in path-to-feature-to-test projection may suggest exact Playwright spec files.
It only uses tests previously imported by Cockpit and requires a local Playwright config
before emitting a runnable action. This is fast-feedback, not a proof of impact completeness.
Explicit execution stores logs locally and leaves existing CI authoritative.

## Monorepo amendment (2026-10-31)

Trying the planner on a monorepo with one Moon workspace, ten Cargo workspaces and one Bun
workspace showed four gaps; the decisions that close them:

- **`--base` means the merge base.** Diffing against the base's tip reported everything the
  base gained after the branch forked as the branch's change. A plan answers "what does this
  change need", so `--base REF` compares with `git merge-base REF HEAD` (pull-request
  semantics). An ancestor of `HEAD` is its own merge base, so a fixed ancestor SHA behaves as
  before; no flag keeps the tip comparison.
- **Path classes instead of a blanket unknown.** Fixtures, golden files, translations and
  checked-in generated files are not source, yet they belong to a project. `[development]`
  names them (`test_data`, with conventional defaults; `generated`; `documentation`;
  `test_inputs` for inputs read across projects); a match plans the owning or named
  project's tests (checks and tests for generated files). Fail-closed stays the rule for
  everything unclassified, and for root configuration (`.github/`, `.moon/`, `tooling/`).
  A root lockfile or manifest plans every project of its language: that is complete for the
  known project set, so the scope is `workspace_wide_path_based`, not `unknown`.
- **Moon decides a project's tasks.** Languages come from `moon.yml` tags or language, so a
  plan never names a task the project lacks; a source file in a project without tasks for its
  language is unmapped.
- **Downstream from declared edges, without Moon.** `downstream = "direct" | "deep"` follows
  `dependsOn` backwards from projects with changed source or generated files. Running
  `moon query --affected --downstream` was rejected: it writes Moon's cache into the worktree
  (planning is read-only) and has hung for many minutes; the `dependsOn` lists are what Moon
  builds its graph from. The expansion is still declared, not compiler-resolved.
- **Edit feedback per Cargo workspace.** Blaze and Cargo run in the workspace that builds the
  changed crate, with `-p` per changed package. Because they are advisory and claim nothing,
  they are offered under an unknown scope too; checkpoint and CI suggestions are not.

`cockpit plan --policy FILE` reads `[development]` from another file for a repository
planned without its own `cockpit.toml`. Plan fields are added, none removed
(`downstream`, `downstream_projects`, `path_classes`, `notes`; ADR-0009).

## Source-config amendment (2026-11-01)

Planning a large module change on the real monorepo found zero actions: one changed file, a
bundle `manifest.json`, was "unknown", and unknown inputs fail closed. Such files are
hand-written configuration of one project, neither test data, generated nor documentation.

- **A fourth class, `source_config`.** `[development] source_config` names globs of project
  configuration. A match belongs to its owning project (the longest-prefix Moon project, as
  for source files) and plans that project's checks and tests, and its dependents under
  `downstream`, exactly like a source file. It is not unknown. A configuration file that is
  source by name (`package.json`, `tsconfig*.json`, `Cargo.toml`) plans its own language only.
- **Defaults are language-agnostic and narrow:** `**/manifest.json`, `**/jsconfig*.json`,
  `**/moon.yml`, `**/moon.yaml`. A listed array replaces the defaults, like `test_data`
  (`[]` turns the class off). Precedence: `test_inputs`, `test_data`, `generated`, then
  `source_config`, so a fixture stays test data.
- **Fail-closed boundaries are unchanged.** Root configuration (`.github/`, `.moon/`,
  `tooling/`, `cockpit.toml`) is decided before any class and stays unknown whatever the
  globs say; a match outside every Moon project (a root `moon.yml`, say) is unmapped, hence
  unknown. The class never widens to every project.
- The plan JSON `path_classes` gains `source_config` (a count; additive).
