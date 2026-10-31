# Agentic SDLC: Rust, TypeScript, Claude Code and GitHub CI

This experimental subsystem is opt-in. Cockpit's main state and health evidence
model remains derived from source and test artifacts.

## Goals

Reduce time and tokens per verified change. Do not optimize away correctness.
The same `cockpit.toml` policy governs recommendations on a developer's machine
and the subset of verification tasks Cockpit can execute on a GitHub runner.

## Config

```toml
[development]
rust_edit_backend = "blaze"         # none | cargo | blaze
rust_ci_backend = "blaze_then_cargo" # cargo | blaze_then_cargo
```

The Rust CI backend is never Blaze-only: Blaze uses its own pinned nightly
and may miss compiler errors. Cargo with the repository's toolchain remains
required. TypeScript runs Moon's `js-check`/`js-test` or Bun package scripts.

## Local Claude Code CLI workflow

```sh
cockpit --json plan --mode edit
cockpit --json usage
cockpit --json plan --mode checkpoint --include-evidence
cockpit --json verify --mode checkpoint
```

Claude Code metrics read only local JSONL message metadata on explicit request;
prompts, tool output and session IDs are not returned. Observed session
spans are not active coding duration and no dollar costs are inferred.

## GitHub Actions adoption

Do not add a duplicate CI lane to an already verified workflow. To *replace*
a generic language-check/test lane, install the appropriate tools and a pinned
Cockpit release, then invoke:

```yaml
- name: Rust and TypeScript verification
  run: cockpit --json verify --mode ci
```

The default CI mode selects all discoverable Moon projects with Cargo.toml
or package.json, or the standalone root project, even when git status is clean.
For repositories that use per-workspace Moon task mappings, the check/test
commands mirror `moon run <project>:rust-check`, `rust-test`, `js-check`, and
`js-test`. When a project has no mapping, the planner does not assert
transitive completeness; review the plan and keep existing gates.

Blaze preflight is optional, an error in the preflight is reported but cannot
replace a pinned Cargo verdict. GitHub CI must still run project-specific
WASM, browser, export/fidelity or security gates not represented by the plan.
The command fails rather than claiming success on an empty or uncertain plan.

## Boundaries

- `plan` is read-only. `verify` explicitly executes commands.
- Prior feature/test hints are historical, not proof of current changes.
- Source file paths and project roots are not a compiler-resolved dependency graph.
- Verification command results do not automatically mark a Cockpit feature healthy.
- Historical Claude usage data remains local; it is never uploaded automatically.
- Research-backed improvement requires measuring full SDLC task latency, tokens
  and correctness against the existing workflow.

## Playwright and change-scoped test feedback

```sh
# Import any existing JUnit/nextest/Playwright results once:
cockpit scan

# Discover historical test candidates and exact Playwright spec-file commands:
cockpit --json plan --mode edit --select-tests --include-evidence

# Explicitly run the selected Playwright specs (never automatically):
cockpit --json verify --mode edit --select-tests
```

Selection joins changed file paths to feature ownership, then to individual
tests linked by the previous Cockpit scan. A modified known spec is also
eligible directly. Playwright is launched only for existing `*.spec.*` files
under a discoverable `playwright.config.*`; test cases within one file collapse
to a single `bunx playwright test <file>` invocation. Incomplete or missing
mappings do **not** prove that other tests are unaffected. This is fast,
advisory edit-time feedback, not authoritative CI.

For a pull request, use a fixed base SHA from `github.event.pull_request.base.sha`
with `--base` when you want to plan changes relative to the PR target
(ensure `fetch-depth: 0`). Use `--mode ci` for project-level validation;
do not substitute the selected Playwright subset for all browser tests,
fidelity checks or the existing required CI workflows.
