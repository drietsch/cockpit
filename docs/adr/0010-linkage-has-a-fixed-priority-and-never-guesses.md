# ADR-0010 — Linkage has a fixed priority and never guesses

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

A test that names its feature must win over any inference; an ambiguous match must not silently pick one.

## Decision

Explicit references (tags, `__feat__`, `[ID]`, interchange `features[]`) → test map → path ownership → unlinked. Ambiguous flattened ids are reported as unresolved references.

## Consequences

Unlinked tests are visible as an informational item per suite, never hidden.
