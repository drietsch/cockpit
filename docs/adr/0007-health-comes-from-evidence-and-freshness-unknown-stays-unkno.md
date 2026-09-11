# ADR-0007 — Health comes from evidence and freshness; unknown stays unknown

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

A dashboard that turns absence of evidence into green teaches people to ignore it.

## Decision

Healthy / Needs Attention / Broken / Unknown are computed from the latest run of each linked test and benchmark, rolled up per category, and compared by commit ancestry against the latest implementation commit. No evidence means Unknown; evidence older than the change means stale; a failing test that predates the change is attention, not broken.

## Consequences

Features can be Unknown for a long time in projects without tests; the headline says so.
