# `cockpit.provenance.v1`

The sidecar `cockpit pull` writes next to a downloaded artifact
(`.cockpit/evidence/<source with '/'→'__'>/<artifact>/provenance.json`). Importers read it for
every file under that directory when the document itself declares no commit or time.

```json
{
  "schema": "cockpit.provenance.v1",
  "source": "core/nextest-unit",
  "repo": "core",
  "commit": "65707ceb5fde63cb676bd6ce5a48dc36c004896c",
  "branch": "main",
  "run": {
    "id": 34583778460,
    "url": "https://github.com/paged-media/core/actions/runs/34583778460",
    "attempt": 1,
    "workflow": "ci.yml",
    "conclusion": "success",
    "event": "push",
    "started_at": "2026-09-11T09:21:26Z",
    "finished_at": "2026-09-11T09:38:32Z"
  },
  "generated_at": "2026-09-11T09:26:31Z",
  "artifact": { "id": 10192863863, "name": "nextest-junit", "size": 58569, "expired": false, "files": ["junit.xml"] },
  "fetched_at": "2026-09-11T13:33:08Z",
  "fetched_by": "cockpit 0.1.0"
}
```

`generated_at` is the artifact's upload time — the moment the results existed — never the time
of the download. Precedence inside an importer: values declared by the document → this sidecar
→ inference (commit by timestamp against the repository's history, time from the file's
mtime), and every choice is recorded as the value's origin.
