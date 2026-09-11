# `cockpit.visual.v1`

Pixel or PDF comparisons as a document, for pipelines that are not Playwright and not the
paged fidelity layout. Read by `format = "cockpit-visual"`.

```json
{
  "schema": "cockpit.visual.v1",
  "source": "core",
  "suite": "fidelity",
  "provenance": { "commit": "…", "generated_at": "2026-09-11T09:38:32Z" },
  "features": ["test-corpus.fidelity-gate"],
  "comparisons": [
    {
      "id": "brochure/page-3",
      "title": "brochure page 3",
      "feature_ids": ["frames-paths.text-wrap"],
      "page": 3,
      "browser": "chromium",
      "viewport": { "width": 1440, "height": 900 },
      "baseline": { "path": "ref/brochure-003.png" },
      "actual": { "path": "cand/brochure-003.png" },
      "diff": { "path": "heat/brochure-003.png" },
      "metrics": { "mean_de": 1.36, "p99_de": 32.5, "ssim": 0.91, "difference_ratio": 0.004 },
      "threshold": { "max_mean_de": 2.83, "max_p99_de": 85.79, "min_ssim": 0.84 },
      "status": "pass"
    }
  ]
}
```

Each comparison becomes one visual test (kind `visual`) with one visual check. `status` is
`pass` · `fail` · `missing_baseline` · `new_baseline` · `inconclusive` · `skipped`; image paths
are relative to the document and served through `/artifacts/…` when they lie inside a
repository or the evidence cache.

The same shape is produced internally from Playwright `-expected/-actual/-diff` attachment
triples and from fidelity directories (`gate.json` + `report.json` + `cand/ref/heat-NNN.png`).
