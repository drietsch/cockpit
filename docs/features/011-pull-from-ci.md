---
id: FEAT-011
title: "Pull from CI"
status: stable
importance: normal
paths:
  - src/pull/**
  - src/cli/pull.rs
adrs:
  - ADR-0012
---

# Pull from CI

Fetching the newest run's artifact for every GitHub-backed evidence source through gh or a token, with a provenance sidecar carrying the run's real completion time.
