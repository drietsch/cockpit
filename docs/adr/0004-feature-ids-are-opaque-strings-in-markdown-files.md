# ADR-0004 — Feature ids are opaque strings in Markdown files

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

Guardian wants `FEAT-NNN`; paged has dotted ids like `layers.ops`. A registry that dictates the shape would lose one of them.

## Decision

A feature is a Markdown file with YAML frontmatter; `id` is any string, optionally constrained by `features.id_pattern`; ids are immutable and renames use `superseded_by`.

## Consequences

Bare ids are only recognised in commit messages and test names when they are distinctive or a pattern is configured; dotted ids are matched via flattened `__feat__` suffixes.
