---
id: FEAT-004
title: "Test evidence importers"
status: stable
importance: critical
paths:
  - src/import/junit.rs
  - src/import/playwright.rs
  - src/import/results_json.rs
  - src/import/batch.rs
  - src/import/cargo_index.rs
  - src/import/sniff.rs
  - src/link/test_link.rs
  - src/link/test_map.rs
adrs:
  - ADR-0009
  - ADR-0010
---

# Test evidence importers

JUnit (nextest, vitest, generic), Playwright JSON and `cockpit.results.v1` / paged-results documents assembled into suites, runs and test runs, with feature linkage by tag, name, test map and path ownership.
