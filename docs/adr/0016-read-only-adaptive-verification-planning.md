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
