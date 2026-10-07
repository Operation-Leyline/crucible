use std::path::PathBuf;
use std::process::Command;

#[test]
fn diff_returns_json_and_nonzero_status_for_a_breaking_contract() {
    let root = std::env::temp_dir().join(format!("crucible-cli-policy-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let old = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../crucible-policy/tests/fixtures/contract.json");
    let mut model: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&old).unwrap()).unwrap();
    model["shapes"]["example.api#GetItem"]["traits"]["smithy.api#http"]["uri"] =
        serde_json::json!("/v2/items");
    let new = root.join("new.json");
    std::fs::write(&new, model.to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_crucible"))
        .args(["diff"])
        .arg(&old)
        .arg(&new)
        .arg("--json")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["rule"] == "compatibility.http")
    );
    let output = Command::new(env!("CARGO_BIN_EXE_crucible"))
        .args(["diff"])
        .arg(&old)
        .arg(&old)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let policy = root.join("crucible.toml");
    std::fs::write(&policy, "schema_version = 1\n[rules.paths]\nprefix = '/v2'").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_crucible"))
        .arg("check")
        .arg(&old)
        .arg("--target")
        .arg("rust")
        .arg("--config")
        .arg(&policy)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("paths.prefix"));
    std::fs::remove_file(new).unwrap();
    std::fs::remove_file(policy).unwrap();
    std::fs::remove_dir(root).unwrap();
}
