mod support;

use cockpit::cli::output::Format;
use std::path::Path;

/// A three-chapter excerpt of a paged-media/state checkout.
fn state_fixture(dir: &Path) {
    let w = |rel: &str, text: &str| {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    w(
        "registry/chapters.yaml",
        "- id: layers\n  title: Layers\n  group: format\n- id: editor-shell\n  title: Editor shell\n  group: editor\n- id: plugin-draw\n  title: paged.draw\n  group: plugin\n",
    );
    w(
        "registry/stages.yaml",
        "- id: core.parser\n  title: Parser\n  order: 1\n- id: editor.panel\n  title: Panels\n  order: 2\n- id: plugin.draw\n  title: paged.draw\n  order: 3\n",
    );
    w(
        "registry/features/layers.yaml",
        "- id: layers.model\n  title: Layer model\n  spec: \"docs: The layer model (/docs/layers)\"\n  stages:\n    core.parser: { status: shipped }\n    editor.panel: { status: partial, note: \"flags drive the panel\" }\n- id: layers.ops\n  title: Layer operations\n  stages:\n    core.parser: { status: partial }\n- id: layers.locking\n  title: Layer locking\n  stages:\n    core.parser: { status: planned }\n",
    );
    w(
        "registry/features/editor-shell.yaml",
        "- id: editor-shell.panels.effects\n  title: Effects\n  since: 2026-06\n  stages:\n    editor.panel: { status: shipped }\n",
    );
    w(
        "registry/features/plugin-draw.yaml",
        "- id: plugin-draw.layers-panel\n  title: Layers panel\n  lifecycle: deprecated\n  superseded_by: plugin-draw.layers-binding-provider\n  stages:\n    plugin.draw: { status: shipped, note: \"RETIRED 2026-08-05\" }\n- id: plugin-draw.layers-binding-provider\n  title: Layers binding provider\n  stages:\n    plugin.draw: { status: shipped }\n",
    );
    w(
        "ingest/suites.yaml",
        "- source: core\n  suite: nextest-unit\n  stage: core.parser\n  display: core cargo-nextest\n- source: editor\n  suite: playwright-e2e\n  stage: editor.panel\n  display: editor Playwright e2e\n",
    );
    w(
        "ingest/artifacts.yaml",
        "- source: core\n  suite: nextest-unit\n  repo: paged-media/core\n  workflow: ci.yml\n  artifact: nextest-junit\n  format: nextest-junit\n- source: editor\n  suite: playwright-e2e\n  repo: paged-media/editor\n  workflow: tests.yml\n  artifact: playwright-results\n  format: playwright-json\n",
    );
    w(
        "ingest/test-map.yaml",
        "- match: { file: \"e2e/layers.spec.ts\" }\n  features: [layers.ops]\n  stage: editor.panel\n",
    );
    w(
        "registry/measurements.yaml",
        "measurements:\n  - id: wasm.app_total_bytes\n    title: App-wide wasm total\n    unit: bytes\n    direction: lower_is_better\n    gate: advisory\n",
    );
    w(
        "ingest/measurements.yaml",
        "measurements:\n  - source: editor\n    lane: wasm-budget\n    repo: paged-media/editor\n    workflow: tests.yml\n    artifact: wasm-budget\n",
    );
}

#[test]
fn migration_is_lossless_and_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    state_fixture(&state);
    let target = tmp.path().join("cockpit");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(
        target.join("cockpit.toml"),
        "[project]\nid = \"paged\"\nname = \"Paged\"\nroot = \"..\"\n",
    )
    .unwrap();
    let fmt = Format { json: false, color: false, ascii: true };
    let out = target.join("docs/features");
    let cfg = target.join("cockpit.toml");
    let code = cockpit::migrate::paged_state::run(
        &state,
        Some(&out),
        Some(&cfg),
        None,
        false,
        false,
        &fmt,
    )
    .unwrap();
    assert_eq!(code, 0);
    let mut files: Vec<String> =
        walk(&out).into_iter().filter(|f| !f.ends_with("_chapter.md")).collect();
    files.sort();
    assert_eq!(files.len(), 6, "{files:?}");
    let effects =
        std::fs::read_to_string(out.join("editor-shell/editor-shell.panels.effects.md")).unwrap();
    assert!(effects.contains("id: editor-shell.panels.effects"));
    assert!(effects.contains("status: stable"), "{effects}");
    assert!(effects.contains("editor.panel:\n    status: shipped"), "{effects}");
    let model = std::fs::read_to_string(out.join("layers/layers.model.md")).unwrap();
    assert!(model.contains("status: stable"), "any shipped claim → stable: {model}");
    assert!(model.contains("note: flags drive the panel"), "{model}");
    let ops = std::fs::read_to_string(out.join("layers/layers.ops.md")).unwrap();
    assert!(ops.contains("status: experimental"), "partial only → experimental: {ops}");
    let locking = std::fs::read_to_string(out.join("layers/layers.locking.md")).unwrap();
    assert!(locking.contains("status: planned"), "{locking}");
    let retired =
        std::fs::read_to_string(out.join("plugin-draw/plugin-draw.layers-panel.md")).unwrap();
    assert!(
        retired.contains("status: deprecated")
            && retired.contains("superseded_by: plugin-draw.layers-binding-provider")
    );
    let config = std::fs::read_to_string(&cfg).unwrap();
    assert!(
        config.contains("# >>> cockpit migrate (paged-state) >>>")
            && config.contains("# <<< end cockpit migrate (paged-state) <<<")
    );
    assert_eq!(config.matches("[[components]]").count(), 3);
    assert!(config.contains("id = \"core/nextest-unit\"") && config.contains("format = \"junit\""));
    assert!(
        config.contains("id = \"editor/playwright-e2e\"")
            && config.contains("format = \"playwright\"")
    );
    assert!(
        config.contains("[evidence.github]") && config.contains("artifact = \"nextest-junit\"")
    );
    assert!(config.contains("[[performance.metrics]]"));
    assert!(target.join("test-map.yaml").exists());
    assert!(config.contains("[project]\nid = \"paged\""), "hand-written head is kept");

    // Run it again: byte-identical output.
    let snapshot: Vec<(String, String)> =
        files.iter().map(|f| (f.clone(), std::fs::read_to_string(out.join(f)).unwrap())).collect();
    let code = cockpit::migrate::paged_state::run(
        &state,
        Some(&out),
        Some(&cfg),
        None,
        false,
        false,
        &fmt,
    )
    .unwrap();
    assert_eq!(code, 0);
    for (f, before) in snapshot {
        assert_eq!(
            std::fs::read_to_string(out.join(&f)).unwrap(),
            before,
            "{f} changed on the second run"
        );
    }
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), config, "config changed on the second run");

    // The migrated project scans: features, components and claims are readable.
    let located = cockpit::config::Config::discover(&target, None).unwrap();
    assert_eq!(located.config.components.len(), 3);
    assert_eq!(
        located.config.evidence.len(),
        3,
        "{:?}",
        located.config.evidence.iter().map(|e| e.id.clone()).collect::<Vec<_>>()
    );
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn rec(base: &Path, d: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                rec(base, &p, out);
            } else if p.extension().map(|x| x == "md").unwrap_or(false) {
                out.push(p.strip_prefix(base).unwrap().to_string_lossy().to_string());
            }
        }
    }
    rec(dir, dir, &mut out);
    out
}
