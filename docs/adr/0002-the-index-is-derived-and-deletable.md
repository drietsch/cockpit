# ADR-0002 — The index is derived and deletable

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

The state Cockpit shows must be reproducible from the repository and its evidence files, or nobody can trust it.

## Decision

`.cockpit/index.sqlite` is a cache plus history. Everything except the history tables (runs, test runs, benchmark runs, visual evidence, attention first-seen, events) is rebuilt on every scan; a schema change drops and recreates the file; `.cockpit/` is never committed.

## Consequences

A wrong index is fixed by deleting it. History that only exists in the index (older runs of overwritten files) is lost with it; that is the accepted price of simplicity.
