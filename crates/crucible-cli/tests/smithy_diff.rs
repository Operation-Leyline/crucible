use std::process::Command;

#[test]
fn smithy_directories_compare_without_a_cargo_project_or_generated_files() {
    let temporary = tempfile::tempdir().unwrap();
    let before = temporary.path().join("before sources");
    let after = temporary.path().join("after sources");
    let model = r#"$version: "2"
namespace example.api
use crucible.protocols#json
use crucible.errors#ValidationException
@json
service Example { version: "1", operations: [GetItem], errors: [ValidationException] }
@http(method: "GET", uri: "/items/{itemId}", code: 200)
operation GetItem { input: GetItemInput, output: GetItemOutput }
@input
structure GetItemInput { @required @httpLabel itemId: String }
@mixin
structure Named { @required name: String }
@output
structure GetItemOutput with [Named] {}
"#;
    for directory in [&before, &after] {
        std::fs::create_dir_all(directory.join("nested")).unwrap();
        std::fs::write(directory.join("nested/api.smithy"), model).unwrap();
    }
    let diff = || {
        Command::new(env!("CARGO_BIN_EXE_crucible"))
            .arg("diff")
            .arg(&before)
            .arg(&after)
            .arg("--json")
            .output()
            .unwrap()
    };
    let identical = diff();
    assert!(
        identical.status.success(),
        "{}",
        String::from_utf8_lossy(&identical.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&identical.stdout).unwrap();
    assert!(report["findings"].as_array().unwrap().is_empty());
    assert_eq!(std::fs::read_dir(&before).unwrap().count(), 1);
    assert_eq!(
        std::fs::read_to_string(before.join("nested/api.smithy")).unwrap(),
        model
    );

    std::fs::write(
        after.join("nested/api.smithy"),
        model.replace("/items/", "/v2/items/"),
    )
    .unwrap();
    let broken = diff();
    assert!(!broken.status.success());
    let report: serde_json::Value = serde_json::from_slice(&broken.stdout).unwrap();
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["rule"] == "compatibility.http")
    );

    std::fs::write(after.join("nested/api.smithy"), "this is not Smithy").unwrap();
    let invalid = diff();
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("cannot assemble"));
}
