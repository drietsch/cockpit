---
id: FEAT-012
title: "Migration from paged-media/state"
status: stable
importance: normal
paths:
  - src/migrate/**
  - src/cli/mod.rs
  - tests/migrate_paged_state.rs
adrs:
  - ADR-0014
---

# Migration from paged-media/state

A lossless, idempotent conversion of a paged-state registry into feature Markdown with claims, plus the configuration fragment for its suites, components and metrics.
