# ADR-0006 — Claims are shown, never counted

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

paged-media/state carried a shipped/partial/planned matrix per component. Those are statements by people, not evidence.

## Decision

Optional `claims:` per feature are rendered as-is; a shipped claim without evidence on its component is an *informational* drift item; claims never change a feature's health state.

## Consequences

Two states, no third: a feature is healthy because of evidence or unknown because there is none.
