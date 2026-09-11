# ADR-0011 — Git through the CLI behind a trait

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

Every developer machine has git; libgit2 and gitoxide add build weight and edge cases without changing what is read.

## Decision

`GitBackend` is a trait; the shipped implementation shells out to `git` with a delimited log format and `--numstat`; repositories are read in parallel.

## Consequences

A future in-process backend can replace it without touching the scanner.
