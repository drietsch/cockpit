# ADR-0013 — Artifacts are served only from closed roots

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

The paged umbrella holds `.env` files and tokens beside the repositories; a naive static file route would expose them.

## Decision

`/artifacts/{root}/…` serves repositories, the evidence cache and explicitly allowed external roots; components, secret-like names and extensions are checked; paths are canonicalised and must stay inside the root; HTML and SVG get a sandbox CSP.

## Consequences

Files outside those roots are unreachable from the UI even when an importer references them.
