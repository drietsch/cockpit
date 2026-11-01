//! `cockpit plan` on a Moon-project fixture: a changed project manifest is planned like source.

use std::fs;
use std::path::Path;
use std::process::Command;

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn plan(root: &Path, file: &str) -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_cockpit"))
        .args(["-C", root.to_str().unwrap(), "--json", "plan", "--mode", "checkpoint", "--file"])
        .arg(file)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn a_changed_manifest_plans_its_project_and_root_config_fails_closed() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    write(
        r,
        "cockpit.toml",
        "[project]\nname = \"fixture\"\n[development]\ndownstream = \"deep\"\n",
    );
    write(r, ".moon/workspace.yml", "projects:\n  draw: modules/draw\n  app: apps/app\n");
    write(r, "modules/draw/moon.yml", "tags: [node]\n");
    write(r, "modules/draw/packages/bundle/manifest.json", "{}");
    write(r, "apps/app/moon.yml", "tags: [node]\ndependsOn: [draw]\n");

    let v = plan(r, "modules/draw/packages/bundle/manifest.json");
    let data = &v["data"];
    assert_eq!(data["scope"], "multiple_projects_path_based");
    assert_eq!(data["path_classes"]["source_config"], 1);
    let commands: Vec<String> = data["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            a["command"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c.as_str().unwrap())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    assert!(commands.contains(&"moon run draw:js-check".to_owned()), "{commands:?}");
    assert!(commands.contains(&"moon run draw:js-test".to_owned()), "{commands:?}");
    assert!(commands.contains(&"moon run app:js-test".to_owned()), "{commands:?}");

    let v = plan(r, ".github/workflows/manifest.json");
    assert_eq!(v["data"]["scope"], "unknown");
    assert!(v["data"]["actions"].as_array().unwrap().is_empty());
}
