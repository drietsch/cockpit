# ADR-0009 — Interchange formats are versioned additively

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

Producers in several languages will write Cockpit documents; breaking them silently is worse than any missing field.

## Decision

`cockpit.results.v1`, `cockpit.benchmark.v1`, `cockpit.visual.v1` and `cockpit.provenance.v1` carry a literal `schema`; v1 only ever gains optional fields; unknown fields are preserved.

## Consequences

A v2 is a new schema literal, read side by side with v1.
