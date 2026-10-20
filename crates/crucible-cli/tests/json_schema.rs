use serde_json::Value;
use std::{fs, process::Command};
mod common;
#[test]
fn cli_selects_a_shape_and_preserves_destination_on_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    common::install_shared(dir.path(), "json-schema");
    let model = dir.path().join("json-schema");
    let output = dir.path().join("schema.json");
    fs::write(dir.path().join("keep.txt"), "consumer-owned").unwrap();
    let run = |shape: &str, audience: &str| {
        Command::new(env!("CARGO_BIN_EXE_crucible"))
            .args(["generate", "json-schema"])
            .arg(&model)
            .args(["--shape", shape, "--audience", audience, "--out"])
            .arg(&output)
            .output()
            .unwrap()
    };
    let result = run("example.schema#RenderPlan", "external");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let previous = fs::read_to_string(&output).unwrap();
    let schema: Value = serde_json::from_str(&previous).unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert!(schema["properties"]["seed"].is_object());
    assert!(schema["$defs"].get("example.schema.Secret").is_none());
    for name in ["NoSuchShape", "bad shape", "Secret"] {
        let result = run(name, "external");
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("JSON Schema shape"));
        assert_eq!(fs::read_to_string(&output).unwrap(), previous);
    }
    assert!(run("Secret", "internal").status.success());
    assert_eq!(
        fs::read_to_string(dir.path().join("keep.txt")).unwrap(),
        "consumer-owned"
    );
}
