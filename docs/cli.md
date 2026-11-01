# cockpit — command line

Global: `-C/--project <path>` (`COCKPIT_PROJECT`), `--json`, `--log <level>` (`COCKPIT_LOG`),
`--no-color`, `--ascii`, `--index <path>`. Exit codes: 0 ok · 1 policy/pull failure · 2
usage/config · 3 no project · 4 index error. `--json` prints exactly one document
`{schema: "cockpit.<cmd>.v1", generated_at, project, data}`.

| Command | What it does |
|---|---|
| `serve [--watch] [--port 4747] [--bind 127.0.0.1] [--open] [--dev] [--no-scan] [--pull-every 15m]` | scan, then serve the UI and the JSON API; `--pull-every` runs `cockpit pull` periodically |
| `scan [--full] [--reset-history]` | import changed files (`--full` re-reads everything; history stays unless reset) |
| `status [--limit N]` | terminal overview: header, counts, attention, recent changes |
| `validate [--strict]` | the gate: ID/FM/REF/ADR/EV/SUITE/PERF/CLAIM checks; exit 1 on errors; `--strict` promotes warnings |
| `export [--profile public\|full] [--out FILE]` | every feature with its lifecycle, claims per component, evidence health and latest implementation change as one `cockpit.export.v1` document, for publishing pipelines; `public` omits commits, attention and benchmarks |
| `feature <id>` | definition of done for one feature (`cockpit.feature.v1` with `--json`) |
| `attention [--all] [--min-severity s] [--feature id] [--limit N]` | ranked attention items (`--all` includes informational) |
| `doctor` | environment and project diagnostics; exit 1 on any ✕ |
| `init [--from-cargo]` | scaffold cockpit.toml, docs/features, docs/adr (never overwrites); `--from-cargo` proposes one feature per crate layer |
| `migrate paged-state <dir> [--out] [--config-out] [--dry-run] [--force]` | 1:1 conversion of a paged-media/state registry |
| `pull [--dry-run] [--force] [--source glob]… [--strict]` | fetch the newest CI artifact per `[evidence.github]` source, then rescan |
| `plan [--file PATH]… [--base REF] [--mode edit|checkpoint|ci] [--fast-backend auto|none|cargo|blaze] [--downstream none|direct|deep] [--policy FILE] [--include-evidence] [--select-tests]` | Read-only changed-file impact and staged verification suggestions (`cockpit.plan.v1` with `--json`); never runs builds or tests |
| `verify [--file PATH]… [--base REF] [--mode checkpoint|ci] [--downstream …] [--policy FILE]` | Explicitly execute policy-selected commands, with bounded output and local logs (`cockpit.verify.v1`) |
| `usage [--transcripts-dir PATH]` | Opt-in, metadata-only Claude Code token/session aggregate (`cockpit.usage.v1` with `--json`) |
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

## Adaptive verification planning (experimental)

```sh
cockpit --json plan --file src/agent.rs --mode checkpoint
cockpit --json plan --mode edit --fast-backend blaze
cockpit plan --base origin/main --mode checkpoint
```

Without `--file`, `plan` inspects changes against `HEAD` and includes untracked non-ignored files. With `--base REF` it compares the worktree with `git merge-base REF HEAD`, as a pull request does: what `REF` gained after the branch forked is not reported as the branch's change (the plan's `source` names the merge base). To compare with a fixed commit, pass one that is an ancestor of `HEAD`. Paths are relative to the selected project root. `--file` and `--base` cannot be combined. The command reads a Moon project map if present and otherwise uses the root Cargo or Bun project. It does not scan the Cockpit evidence index, run a build, run tests, or invoke Blaze. `--fast-backend blaze` only proposes advisory rapid Rust feedback; it does not replace the project's pinned Cargo toolchain. Checkpoint mode proposes checks and tests for owning projects. Unknown/configuration inputs prevent claiming scoped checks are sufficient. `--mode ci` performs no local work and defers to existing CI. Path ownership is not semantic dependency analysis; all recommendations are **unverified** until executed and the authoritative CI gates are evaluated.

### Monorepos

- **Languages** of a Moon project come from its `moon.yml` `tags` (`rust`; `node`, `bun`, `typescript`…), else its `language`, else its `Cargo.toml`/`package.json`; a project gets `<id>:rust-check`/`rust-test` and `<id>:js-check`/`js-test` only for the languages it has. A source file whose owning project has no tasks for its language is unmapped (scope `unknown`), with a note.
- **Path classes** from `[development]` (`test_data`, `generated`, `source_config`, `documentation`, `test_inputs`; see [configuration](config.md)) keep fixtures, translations and checked-in generated files from making every plan `unknown`. `source_config` (project manifests and `moon.yml` by default) plans the owning project like a source file. Counts are in `path_classes`. A root lockfile or manifest (`bun.lock`, root `package.json`, `Cargo.lock`, `rust-toolchain.toml`…) plans every Moon project of that language (scope `workspace_wide_path_based`). Other root configuration (`.github/`, `.moon/`, `tooling/`) still makes the scope `unknown`.
- **Downstream:** `--downstream direct|deep` (or `[development] downstream`) adds the projects that depend on a changed project, read from each project's `moon.yml` `dependsOn` (`downstream_projects`). Test-data changes do not expand. Cockpit does not run `moon query`: it writes Moon's cache into the worktree and can take minutes; the declared edges are what Moon's own graph is built from, but they are not a compiler-resolved graph.
- **Edit-time Rust feedback** runs in the Cargo workspace that builds each changed crate (the nearest `[workspace]` manifest above it that does not exclude it), with `-p <package>` per changed package, `--workspace` for a workspace-level manifest or lockfile, and `--all-targets` when a changed file is a test. It is advisory, so it is offered even when the scope is `unknown`; checkpoint and CI suggestions are not. Without Moon, each Cargo workspace is a project of its own.
- **`--policy FILE`** reads `[development]` from another `cockpit.toml` (only that section), for a repository planned without its own config.

Add `--include-evidence` to join changed paths to feature `paths:` globs and linked tests from the **last existing Cockpit scan** without rescanning or executing anything. Results are capped (10 features, 12 tests) and explicitly marked historical; prior health and test statuses do not verify changed code. No scan returns `unavailable_not_scanned`, not a green result.

## Shared development and CI policy

The optional `[development]` section of `cockpit.toml` governs both local planning and CI suggestions:

```toml
[development]
rust_edit_backend = "blaze"     # none | cargo | blaze (default: none)
rust_ci_backend = "blaze_then_cargo" # cargo | blaze_then_cargo (default: cargo)
```

`cockpit plan --mode edit` uses the configured Rust feedback backend, overridable with `--fast-backend none|cargo|blaze`. `cockpit plan --mode ci` always proposes the pinned-Cargo checks and tests for known Rust projects. When configured, Blaze is a *preflight in addition to Cargo*, never an alternative authoritative result. TypeScript uses Moon's `js-check`/`js-test` or Bun scripts. The same checked-in policy can be read in a GitHub Actions job; **existing CI workflows must explicitly run the planned commands**. No workflow is changed or invoked by the planning command itself. An empty working-tree change list in CI means consider all detectable projects; it is not treated as proof that nothing changed.

## Claude Code usage metrics

```sh
cockpit --json usage
cockpit -C /path/to/repo --json usage --transcripts-dir /path/to/claude/projects
```

This command scans local `~/.claude/projects` transcript JSONL on demand, counting only assistant records whose cwd is within the chosen repo. It aggregates model, input/output/cache tokens and observed session spans; it does **not** export prompts, tools, full transcript contents, raw message IDs or file paths. Duplicate streaming messages are deduplicated per transcript. It never estimates dollar costs from unverifiable prices, and observed session span is not active coding time. Local transcript formats can change; missing/unrecognized records are not fabricated. It does not change feature health, and is not uploaded to GitHub automatically.

## Executing the policy (opt-in)

`cockpit plan` never starts processes. `cockpit verify` explicitly executes its
checkpoint/CI recommendations, captures complete local logs in `.cockpit/verification`,
and prints only short results or failure tails. It stops on the first non-advisory
failure (exit 1); an optional Blaze preflight is advisory. Unknown change impact,
missing executable, path escape, or an empty plan fail closed rather than
reporting success. Only the selected project checks are marked passed. Existing
repository-specific GitHub gates, WASM, browser and cross-workspace checks are
**not** replaced by this command.

```sh
cockpit verify --mode checkpoint --file src/lib.rs
cockpit --json verify --mode ci
```

For GitHub Actions, the same `cockpit.toml` policy is read from the checkout:
install the pinned Cockpit binary, install the repository's required Rust/Blaze,
Moon and Bun toolchains, and run `cockpit --json verify --mode ci` in a job.
This invokes the discovered projects' checks and tests even in a clean CI
checkout. Keep existing specialized CI jobs; **do not duplicate checks** when
adopting this as their replacement. The CI checkout must provide a complete
project mapping, otherwise verification fails rather than claiming coverage.

The CLI command does not create or edit GitHub workflows. See
`docs/agentic-sdlc.md` for adoption and limitations.

## Impact-driven test selection (experimental)

```sh
cockpit --json plan --file apps/editor/src/login.tsx --mode edit --select-tests
cockpit --json verify --file apps/editor/src/login.tsx --mode edit --select-tests
```

Test selection uses existing Cockpit feature `paths:` ownership and test links from the **last scan**. Changes directly to a recorded spec file also make it a candidate. No scanned test inventory means `unavailable_not_scanned`, not an empty proof. The machine-readable plan reports candidates, prior status, reasons, truncation, and exact runnable actions independently.

Only existing `*.spec.ts(x)`/`*.spec.js(x)`/ `*.spec.mts` Playwright files with a discoverable `playwright.config.*` ancestor become executable edit-time actions. Multiple test cases in one spec are grouped to one `bunx playwright test <file>` invocation. This is **advisory targeted feedback**; it never proves other tests unaffected. `cockpit verify --mode edit --select-tests` is explicit execution and fails when no exact runnable tests are available; checkpoint and CI still run broader project checks/tests. Neither Playwright nor feature coverage is inferred from prior passing status. Unknown impacted code still requires normal verification.

The verifier writes full logs to `.cockpit/verification/` and returns bounded error summaries, not huge model-context output. No test is run by `plan`.
