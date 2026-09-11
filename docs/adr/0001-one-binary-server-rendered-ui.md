# ADR-0001 — One binary, server-rendered UI

**Status:** Accepted (2026-09-11)
**Date:** 2026-09-11

## Context

Cockpit must run anywhere a repository is checked out, without Node, a database server or a browser build step.

## Decision

A single Rust binary embeds the templates and vendored assets (htmx, idiomorph, ECharts) and serves them itself. Pages are rendered on the server with MiniJinja; htmx swaps `<main>`; JavaScript is limited to small ES modules for live updates, charts, the compare viewer and keyboard navigation.

## Consequences

No frontend toolchain to install or keep current; every page works without JavaScript except charts and the viewer. Rich client-side interactivity stays deliberately small.
