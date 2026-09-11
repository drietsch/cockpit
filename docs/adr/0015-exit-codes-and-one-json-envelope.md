# ADR-0015 — Exit codes and one JSON envelope

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

Scripts and agents call the CLI; they need stable contracts more than pretty output.

## Decision

0 ok · 1 policy or pull failure · 2 usage or configuration · 3 no project · 4 index error; `--json` prints exactly one document `{schema: "cockpit.<cmd>.v1", generated_at, project, data}`; errors are `cockpit.error.v1`.

## Consequences

Terminal output may change freely; JSON only grows.
