# ADR-0014 — Migration is lossless and idempotent

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

paged-media/state had 580 curated rows with long notes; a migration that summarised them would be a rewrite nobody asked for.

## Decision

`cockpit migrate paged-state` writes one feature file per row with every claim and note verbatim, a `migrated_hash` for idempotency, and a marked configuration block that can be regenerated; hand-edited files are kept.

## Consequences

The parity script in paged/cockpit checks the migration against the registry before the old system is archived.
