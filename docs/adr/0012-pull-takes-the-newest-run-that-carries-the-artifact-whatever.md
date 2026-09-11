# ADR-0012 — Pull takes the newest run that carries the artifact, whatever its conclusion

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

A failed CI run with a JUnit report is exactly the evidence that matters; filtering on `success` hides red.

## Decision

`cockpit pull` walks completed runs on the branch newest first and takes the first with an unexpired artifact of the configured name; the sidecar records the run, the commit and the artifact's upload time as `generated_at`.

## Consequences

An artifact absent from the last N runs is a skip with a reason, not an error.
