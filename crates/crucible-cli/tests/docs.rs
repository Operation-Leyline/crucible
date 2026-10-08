use std::fs;
use std::process::Command;

use serde_json::Value;

#[test]
fn docs_cli_projects_audiences_and_preserves_outputs_on_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let model = root.join("model.json");
    let out = root.join("reference");
    fs::write(
        &model,
        include_str!("../../crucible-smithy/tests/fixtures/service-docs-weather.json"),
    )
    .unwrap();
    let run = |audience: &str| {
        Command::new(env!("CARGO_BIN_EXE_crucible"))
            .args(["generate", "docs"])
            .arg(&model)
            .arg("--out")
            .arg(&out)
            .args(["--audience", audience])
            .output()
            .unwrap()
    };
    let result = run("internal");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(out.join("src/operations/API_Status.md").is_file());
    fs::write(out.join("keep.txt"), "consumer-owned").unwrap();
    let result = run("external");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!out.join("src/operations/API_Status.md").exists());
    let bytes = fs::read(out.join("describe.json")).unwrap();
    let describe: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(describe["format"], "crucible.describe/1");
    assert!(describe["resources"]["example.weather#City"].is_object());
    assert_eq!(
        fs::read_to_string(out.join("keep.txt")).unwrap(),
        "consumer-owned"
    );
    fs::write(
        &model,
        include_str!("../../crucible-smithy/tests/fixtures/service-docs-unsupported.json"),
    )
    .unwrap();
    let result = run("external");
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("describe.protocol"));
    assert_eq!(fs::read(out.join("describe.json")).unwrap(), bytes);
}
