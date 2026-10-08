#![cfg(feature = "rust")]

use std::path::PathBuf;

use crucible_build::model;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../crucible-policy/tests/fixtures/contract.json")
}

#[test]
fn explicit_policy_is_required_and_applied_before_generation() {
    let root = std::env::temp_dir().join(format!("crucible-build-policy-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("crucible.toml");
    let codegen = model(fixture()).server().custom_rejections().policy(&path);
    assert!(codegen.render().unwrap_err().contains("cannot read"));
    std::fs::write(&path, "schema_version = 1\n[rules.paths]\nprefix = '/v2'").unwrap();
    assert!(codegen.render().unwrap_err().contains("paths.prefix"));
    std::fs::write(&path, "schema_version = 1\n[rules.paths]\nprefix = '/v1'").unwrap();
    assert!(codegen.render().is_ok());
    std::fs::write(
        &path,
        "schema_version = 1\n[rules.paths]\nprefix = '/v2'\nseverity = 'warning'",
    )
    .unwrap();
    assert!(codegen.render().is_ok());
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&root).unwrap();
}
