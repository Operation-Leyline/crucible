use std::fs;
use std::process::Command;

const MALFORMED: &str =
    include_str!("../../crucible-smithy/tests/fixtures/protocol-malformed-requests.json");

#[test]
fn client_export_leaves_out_server_only_cases() {
    let directory =
        std::env::temp_dir().join(format!("crucible-conformance-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let model = directory.join("model.json");
    fs::write(&model, MALFORMED).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_crucible"))
        .args(["conformance"])
        .arg(&model)
        .args(["--target", "python", "--out"])
        .arg(directory.join("out"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let exported: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(directory.join("out/protocol-tests.json")).unwrap(),
    )
    .unwrap();
    let tests = exported["tests"].as_array().unwrap();
    assert!(
        tests.iter().all(|test| test["appliesTo"] != "server" && test["case"]["kind"] != "malformedRequest"),
        "{tests:?}"
    );
    fs::remove_dir_all(&directory).unwrap();
}
