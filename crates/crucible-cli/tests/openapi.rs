use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

fn temporary(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "crucible-openapi-{}-{name}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&path).unwrap();
    path
}

#[test]
fn cli_writes_a_spec_file_and_preserves_sibling_files() {
    let directory = temporary("success");
    let model = directory.join("model.json");
    let output = directory.join("openapi.json");
    fs::write(&model, WEATHER).unwrap();
    fs::write(directory.join("keep.txt"), "consumer-owned").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_crucible"))
        .args(["generate", "openapi"])
        .arg(&model)
        .arg("--out")
        .arg(&output)
        .args([
            "--endpoint",
            "https://api.example.com/v1",
            "--service",
            "example.weather#Weather",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: Value = serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(doc["servers"][0]["url"], "https://api.example.com/v1");
    assert_eq!(doc["paths"]["/cities"]["post"]["operationId"], "CreateCity");
    assert_eq!(
        fs::read_to_string(directory.join("keep.txt")).unwrap(),
        "consumer-owned"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn cli_diagnostics_do_not_replace_an_existing_document() {
    let directory = temporary("rejected");
    let model = directory.join("model.json");
    let output = directory.join("openapi.json");
    let mut value: Value = serde_json::from_str(WEATHER).unwrap();
    value["shapes"]["example.weather#Status"]["traits"]["smithy.api#http"]["uri"] =
        json!("/ping?mode=status");
    fs::write(&model, value.to_string()).unwrap();
    fs::write(&output, "previous document").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_crucible"))
        .args(["generate", "openapi"])
        .arg(&model)
        .arg("--out")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("multiple operations on GET /ping"));
    assert_eq!(fs::read_to_string(&output).unwrap(), "previous document");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn cli_forwards_the_explicit_protocol_selection() {
    let directory = temporary("protocol");
    let model = directory.join("model.json");
    let output = directory.join("openapi.json");
    let mut value: Value = serde_json::from_str(WEATHER).unwrap();
    value["shapes"]["example.weather#Weather"]["traits"]["aws.protocols#restJson1"] = json!({});
    fs::write(&model, value.to_string()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_crucible"))
        .args(["generate", "openapi"])
        .arg(&model)
        .arg("--out")
        .arg(&output)
        .args(["--protocol", "aws.protocols#restJson1"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: Value = serde_json::from_str(&fs::read_to_string(output).unwrap()).unwrap();
    assert_eq!(doc["x-smithy-protocol"], "aws.protocols#restJson1");
    fs::remove_dir_all(directory).unwrap();
}
