# ADR-0005 — ADRs are read in the dialects that exist

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

Guardian writes `**Status:** Accepted (date)` with wrapping lines; paged writes `· status: ACCEPTED` inline or `- **Status:**` lists; nobody will rewrite 80 documents for a tool.

## Decision

The ADR importer accepts frontmatter, bold-label headers and inline status lines, normalises the vocabulary (accepted/ratified/approved → Accepted, …) and reports what it could not read as a warning rather than guessing.

## Consequences

New dialects are added to the importer, never to the documents.
