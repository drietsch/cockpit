---
id: FEAT-013
title: "Agentic SDLC planning"
status: experimental
importance: high
paths:
  - src/agent.rs
  - src/agent_workspace.rs
  - src/cli/plan.rs
  - src/cli/mod.rs
adrs:
  - ADR-0016
---

# Agentic SDLC planning

Read-only, bounded guidance for verification after source changes. The goal is
lower end-to-end agent time and token consumption without reducing verified
correctness or changing existing feature-health semantics.

## Initial acceptance criteria

- `cockpit plan --file PATH --mode edit|checkpoint|ci --json` returns one
  `cockpit.plan.v1` document with input provenance and unexecuted suggestions.
- Without explicit files, inspect staged, unstaged and untracked changes
  relative to HEAD or an explicit base.
- Map file paths to Moon project owners using longest matching project root.
  A repo without Moon uses a root Cargo/Bun project when available.
- Never run a check, test or build as a side effect of planning.
- Documentation-only changes should not trigger local code checks by default.
- Root configuration or unmapped source changes explicitly mark scope unknown.
- Optional Blaze feedback must be labelled advisory, with Cargo/CI retained as
  the authoritative compatibility gate.
- No plan may introduce healthy evidence into Cockpit's state engine.
- `--base REF` diffs from `git merge-base REF HEAD`, never from the tip of `REF`.
- Configured test-data, generated, documentation and test-input path classes map such
  files to the owning (or named) project's tests; unclassified non-source files stay unknown.
- A Moon project's languages come from its `moon.yml` tags or language, not from the
  changed file's extension; a root lockfile plans every project of its language.
- Optional downstream expansion follows `moon.yml` `dependsOn` backwards (direct or deep),
  without starting Moon.
- Edit-time Rust feedback runs in each changed crate's Cargo workspace with `-p <crate>`.
- Optional `--include-evidence` returns bounded prior feature/test hints from the existing Cockpit scan, clearly marked historical rather than verified against current edits.

## Deferred work

Compiler-resolved impact graph, exact test selection, dependency-aware CI
planning, reliable cache reuse, agent context selection, execution, and
benchmarks against completed real development tasks.

## Rust/TypeScript and CI policy

An optional `[development]` section specifies the Rust edit-time feedback engine and
whether GitHub CI should optionally run a Blaze preflight before mandatory Cargo.
TypeScript uses Moon/Bun; planning never executes commands.

## Claude Code consumption metrics

A separate opt-in `cockpit usage` CLI aggregates input/output/cache token usage,
model and observed session spans from local Claude Code transcripts. It excludes
prompt/tool content, never records inferred billing costs, and preserves Cockpit's
health/evidence isolation.

## Explicit execution

`cockpit verify` executes the staged plan only when explicitly invoked. It writes complete
local logs under `.cockpit/verification` and emits a bounded result. An unrecognized
impact scope is an error. GH Actions can call the same policy-backed command, but
existing specialized CI gates remain separately authoritative.

## Impact-driven tests

Read-only optional `--select-tests` exposes previous scan's feature-linked test candidates,
prior status and exact executable Playwright spec-file actions when config discovery succeeds.
Only explicit `cockpit verify --mode edit --select-tests` runs that subset.
Rust nextest and TypeScript suites remain in checkpoint and CI paths.
Selection is not exhaustive code coverage; missing mappings and past green results
do not justify skipping authoritative checks.
