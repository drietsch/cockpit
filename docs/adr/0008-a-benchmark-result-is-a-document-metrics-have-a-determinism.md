# ADR-0008 — A benchmark result is a document; metrics have a determinism class

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

Guardian's performance contract (ADR-0057 there) already established that a number without provenance cannot be compared and that only deterministic metrics may gate.

## Decision

`cockpit.benchmark.v1` carries commit, tree state, machine and workload; runs compare only within one environment class; deterministic changes gate, semi-deterministic ones within a band, environment-sensitive ones are reported and never gate; accepted regressions are informational.

## Consequences

Incomparable is a first-class outcome, shown with both environments and no percentage.
