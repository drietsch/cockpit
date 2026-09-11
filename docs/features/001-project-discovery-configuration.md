---
id: FEAT-001
title: "Project discovery and configuration"
status: stable
importance: critical
paths:
  - src/config.rs
  - src/paths.rs
  - src/cli/init.rs
adrs:
  - ADR-0003
---

# Project discovery and configuration

Finding the project (`cockpit.toml`, `cockpit/cockpit.toml`, or an implicit conventional project at the nearest `.git`), parsing the configuration with `deny_unknown_fields`, resolving evidence patterns, and scaffolding a new project with `cockpit init`.
