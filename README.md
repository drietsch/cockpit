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

Docs: [configuration](docs/config.md) · [command line](docs/cli.md) · formats:
[results](docs/formats/cockpit.results.v1.md), [benchmark](docs/formats/cockpit.benchmark.v1.md),
[visual](docs/formats/cockpit.visual.v1.md), [provenance](docs/formats/cockpit.provenance.v1.md) ·
[decisions](docs/adr) · [the briefing](docs/briefing.md).

Cockpit dogfoods itself: `cockpit.toml` at this root reads its own features, ADRs and nextest
report.
