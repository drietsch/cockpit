# cockpit — command line

Global: `-C/--project <path>` (`COCKPIT_PROJECT`), `--json`, `--log <level>` (`COCKPIT_LOG`),
`--no-color`, `--ascii`, `--index <path>`. Exit codes: 0 ok · 1 policy/pull failure · 2
usage/config · 3 no project · 4 index error. `--json` prints exactly one document
`{schema: "cockpit.<cmd>.v1", generated_at, project, data}`.

| Command | What it does |
|---|---|
| `serve [--watch] [--port 4747] [--bind 127.0.0.1] [--open] [--dev] [--no-scan]` | scan, then serve the UI and the JSON API |
| `scan [--full] [--reset-history]` | import changed files (`--full` re-reads everything; history stays unless reset) |
| `status [--limit N]` | terminal overview: header, counts, attention, recent changes |
| `validate [--strict]` | the gate: ID/FM/REF/ADR/EV/SUITE/PERF/CLAIM checks; exit 1 on errors; `--strict` promotes warnings |
| `feature <id>` | definition of done for one feature (`cockpit.feature.v1` with `--json`) |
| `attention [--all] [--min-severity s] [--feature id] [--limit N]` | ranked attention items (`--all` includes informational) |
| `doctor` | environment and project diagnostics; exit 1 on any ✕ |
| `init [--from-cargo]` | scaffold cockpit.toml, docs/features, docs/adr (never overwrites); `--from-cargo` proposes one feature per crate layer |
| `migrate paged-state <dir> [--out] [--config-out] [--dry-run] [--force]` | 1:1 conversion of a paged-media/state registry |
| `pull [--dry-run] [--force] [--source glob]… [--strict]` | fetch the newest CI artifact per `[evidence.github]` source, then rescan |
| `query '<expr>' [--format table\|ids\|count\|json]` | the query language (below) |
| `completions <shell>` | bash, zsh, fish, powershell, elvish |

## Query language

```
[features|tests|benchmarks|adrs where] <expr> [order by <field> [asc|desc]] [limit <n>]
<expr> := <term> (and|or <term>)*      <term> := not <term> | ( <expr> ) | <field> <op> <value>
<op>   := = != ~ > >= < <= in has        <value> := word | "string" | number | 7d | (a, b, c)
```

Feature fields: `id title state confidence importance status tag owner adr path group component
claim.<component> evidence.<unit|integration|e2e|visual|performance> changed_since attention
tests benchmarks visual performance.regression`. Test fields: `id name status kind suite feature
flaky duration path tag component last_run runs feature_state`. Benchmark fields: `id name metric
subject state severity determinism unit value change feature env last_run`.

```sh
cockpit query 'state = unknown and importance in (critical, high)'
cockpit query 'features where claim.core.parser = shipped and evidence.unit != pass'
cockpit query 'tests where status = failed order by name limit 20' --format ids
cockpit query 'benchmarks where state = regression and determinism = deterministic'
cockpit query 'changed_since >= 7d and not state = healthy'
```

The same grammar backs `GET /api/v1/query?q=…` and `GET /api/v1/features?query=<expr>`.
