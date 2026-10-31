# Project Cockpit

A local-first engineering instrument panel: one Rust binary that reads a repository's feature
specs, architecture decision records, test reports, benchmark documents, visual comparisons
and Git history — and derives, never asserts, whether each feature currently works.

```
STATE → ATTENTION → CHANGE → FEATURE → REASON → EVIDENCE
```

- **Features** are Markdown files with YAML frontmatter (`docs/features/*.md`); their `paths:`
  decide which tests and commits belong to them.
- **Evidence** is JUnit (nextest, vitest, generic), Playwright JSON, `cockpit.results.v1`,
  benchmark documents (`cockpit.benchmark.v1`, Guardian ledgers, Criterion), fidelity-diff
  directories and `cockpit.visual.v1` — read from the tree or pulled from GitHub Actions.
- **Health** is Healthy / Needs Attention / Broken / Unknown, computed from the latest run of
  every linked test and benchmark and from whether that evidence postdates the latest
  implementation commit. No evidence is Unknown, not green.
- One binary, no Node, no database server: SQLite index in `.cockpit/`, server-rendered UI
  with htmx, charts on demand.

## Agentic development (experimental)

Cockpit also helps **Claude Code CLI** work through the edit → verify → continue loop
with less unnecessary build/test work and smaller tool responses. This is **opt-in**,
separate from Cockpit's derived feature-health/evidence model, and optimized first
for **Rust, TypeScript and Playwright**.

- **Change-aware planning:** `cockpit plan` uses Git changes, Moon project ownership
  (or standalone Cargo/Bun), and optional feature/test links from the last scan to
  recommend checks in **edit**, **checkpoint**, or **CI** mode. Plans never execute checks.
- **Selective Playwright feedback:** `--select-tests` finds candidate spec files
  from previously linked tests or changed test files, then deduplicates file-level
  Playwright commands. It is advisory—not complete coverage or a replacement for CI.
- **Explicit verification:** `cockpit verify` runs the chosen commands on request,
  retaining full local logs while returning compact outcomes and durations.
- **Claude Code usage:** `cockpit usage` reads local Claude Code session metadata
  on request and reports model-level input, output and cache token totals. It does
  not export prompts, tool contents or session IDs, or invent billing estimates.
- **Shared GitHub CI policy:** optional Rust fast feedback via Blaze; Cargo remains
  authoritative. Rust and TypeScript checkpoint/CI plans use Cargo/Bun or Moon tasks.
  GitHub Actions must explicitly invoke Cockpit; existing required gates stay in place.

```sh
cockpit --json plan --mode edit --select-tests --include-evidence
cockpit --json verify --mode edit --select-tests  # selected Playwright feedback, if available
cockpit --json verify --mode checkpoint          # wider project-level checks/tests
cockpit --json usage                             # local Claude Code token summary
```

Optional shared policy in `cockpit.toml`:

```toml
[development]
rust_edit_backend = "blaze"          # none | cargo | blaze
rust_ci_backend = "blaze_then_cargo" # cargo | blaze_then_cargo
```

For GitHub CI, once Cockpit and the relevant build tools are installed, use
`cockpit --json verify --mode ci` alongside any required project-specific
browser, WASM, fidelity, security and deployment gates. **Neither historical
passing results nor selected tests prove that a current change is fully verified.**
For configuration, caveats and CI examples, see [agentic SDLC](docs/agentic-sdlc.md).

## Try it

```sh
cargo install --path .          # or a release binary
cd your-repo
cockpit init                    # cockpit.toml + docs/features scaffold (never overwrites)
cargo nextest run --profile ci  # or whatever produces your JUnit / Playwright reports
cockpit status                  # terminal overview
cockpit serve --watch           # http://127.0.0.1:4747
cockpit validate --strict       # the gate
```

Docs: [configuration](docs/config.md) · [command line](docs/cli.md) · [agentic SDLC](docs/agentic-sdlc.md) · formats:
[results](docs/formats/cockpit.results.v1.md), [benchmark](docs/formats/cockpit.benchmark.v1.md),
[visual](docs/formats/cockpit.visual.v1.md), [provenance](docs/formats/cockpit.provenance.v1.md) ·
[decisions](docs/adr) · [the briefing](docs/briefing.md).

Cockpit dogfoods itself: `cockpit.toml` at this root reads its own features, ADRs and nextest
report.
