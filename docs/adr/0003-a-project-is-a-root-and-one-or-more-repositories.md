# ADR-0003 — A project is a root and one or more repositories

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

Paged is an umbrella folder of sibling clones; Guardian is one repository. Both must be one Cockpit project.

## Decision

All paths are project-root-relative; commits are keyed by (repo, sha); repositories are auto-discovered to depth two or listed under `[[repos]]`; `cockpit.toml` may live in a sub-directory (`cockpit/cockpit.toml`) with `root = ".."`.

## Consequences

Freshness compares commits within one repository only; cross-repository ancestry is not attempted.
