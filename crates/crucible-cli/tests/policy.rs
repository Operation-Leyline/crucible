mod common;

use std::process::Command;

#[test]
fn check_applies_an_explicit_policy_to_a_package() {
    let root = std::env::temp_dir().join(format!("crucible-cli-policy-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let model = common::shared_package("policy-contract");
    let policy = root.join("crucible.toml");
    std::fs::write(&policy, "schema_version = 1\n[rules.paths]\nprefix = '/v2'").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_crucible"))
        .arg("check")
        .arg(&model)
        .arg("--target")
        .arg("rust")
        .arg("--config")
        .arg(&policy)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("paths.prefix"));
    std::fs::remove_file(policy).unwrap();
    std::fs::remove_dir(root).unwrap();
}
