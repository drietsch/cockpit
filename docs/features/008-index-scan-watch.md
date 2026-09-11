---
id: FEAT-008
title: "Index, scan and watch"
status: stable
importance: critical
paths:
  - src/store/**
  - src/scan/**
  - src/state/build.rs
  - src/state/mod.rs
  - src/cockpit.rs
  - tests/scan_end_to_end.rs
  - tests/support/**
adrs:
  - ADR-0002
---

# Index, scan and watch

The SQLite index (derived, deletable; history tables survive rescans), the incremental scanner with one tree walk for all evidence globs, currency rules, and the polling watcher.
