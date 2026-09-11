---
id: FEAT-002
title: "Git history and change correlation"
status: stable
importance: critical
paths:
  - src/git/**
  - src/link/commit_link.rs
  - src/link/ownership.rs
  - src/link/refs.rs
adrs:
  - ADR-0003
  - ADR-0011
---

# Git history and change correlation

Reading every repository's history through the git CLI, keeping an in-memory commit graph for ancestry, and attributing commits to features by explicit ids and path ownership.
