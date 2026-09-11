---
id: FEAT-007
title: "Health, freshness and attention"
status: stable
importance: critical
paths:
  - src/state/health.rs
  - src/state/freshness.rs
  - src/state/rollup.rs
  - src/state/attention.rs
  - src/state/test_history.rs
  - src/state/events.rs
  - src/model/health.rs
  - src/model/attention.rs
  - tests/health_end_to_end.rs
adrs:
  - ADR-0007
---

# Health, freshness and attention

Per-category rollups, change-freshness against the latest implementation commit, the four health states with reasons and a why-chain, ranked attention items with preserved first-seen times, and the engineering timeline.
