# `cockpit.results.v1`

Test results as a document: one file per run, with the provenance that lets Cockpit place it
in history and compare it against the implementation commit. Read by `format = "cockpit-results"`
(and `paged-results` for the paged `contract: 1` predecessor, which has the same body).

```json
{
  "schema": "cockpit.results.v1",
  "source": "core",
  "suite": "conformance",
  "kind": "conformance",
  "component": "core.parser",
  "provenance": {
    "commit": "65707ceb5fde63cb676bd6ce5a48dc36c004896c",
    "branch": "main",
    "generated_at": "2026-09-11T09:38:32Z",
    "tool": { "name": "paged-conformance", "version": "1.4.0" },
    "run": { "id": "34583778460", "url": "https://github.com/…/actions/runs/34583778460" }
  },
  "results": [
    {
      "test_id": "idml/anchored-objects::parsed",
      "title": "anchored objects parse",
      "file": "corpus/idml/anchored-objects.idml",
      "features": ["frames-paths.anchored-objects"],
      "status": "passed",
      "duration_ms": 120,
      "level": "parsed",
      "artifacts": [{ "type": "log", "path": "logs/anchored-objects.txt" }]
    }
  ]
}
```

| Field | Required | Meaning |
|---|---|---|
| `schema` | yes | `cockpit.results.v1` |
| `source`, `suite` | yes | together they name the evidence source (`<source>/<suite>`); when that id is configured in `cockpit.toml`, the document belongs to it regardless of which glob matched the file |
| `kind` | no | `unit` · `integration` · `e2e` · `visual` · `conformance` (default: the source's `kind`) |
| `provenance.commit` | strongly recommended | the commit the results were produced from; without it freshness is unknown |
| `provenance.generated_at` (v1 flat: `finished_at`) | recommended | real completion time; else the file's mtime is used and recorded |
| `results[].test_id` | yes | stable identity within the suite |
| `results[].status` | yes | `passed` · `failed` · `skipped` · `flaky` |
| `results[].features` | no | feature ids this result supports (explicit linkage, highest priority) |
| `results[].component` (alias `stage`), `level` (alias `coverage_level`) | no | component attribution; `[evidence] component_by_level` maps levels to components |
| `results[].artifacts[]` | no | `{type: screenshot|video|trace|log|expected|actual|diff|other, path (relative to the document) | url, mime?, label?}` |
| `results[].error` | no | `{message, stack}` |

An empty `results` array is a **silent** run (the suite ran with no inputs): never green, never
red. A run in which every result failed and there are at least `all_failed_min_tests` results
is treated as invalid when `[evidence.validity] treat_all_failed_as_invalid` is set.
