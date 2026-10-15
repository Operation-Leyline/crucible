use std::path::Path;
use std::process::Command;

const MANIFEST: &str = "schema_version = 1\n[model]\nnamespace = \"example.api\"\nlanguage = 1\nsources = [\".\"]\nimports = [\"crucible.errors\"]\n[model.naming]\ninput = \"{Operation}Input\"\noutput = \"{Operation}Output\"\n";

const MODEL: &str = r#"service Example {
    version "1"
    protocol json
    operations [GetItem]
    errors [ValidationException]
}

readonly operation GetItem {
    http GET "/items/{itemId}" -> 200

    input {
        itemId: string
    }

    output {
        name: string
    }
}
"#;

fn package(directory: &Path, model: &str) {
    std::fs::create_dir_all(directory.join("nested")).unwrap();
    std::fs::write(directory.join("crucible.toml"), MANIFEST).unwrap();
    std::fs::write(directory.join("nested/api.cru"), model).unwrap();
}

#[test]
fn package_directories_compare_without_a_cargo_project_or_generated_files() {
    let temporary = tempfile::tempdir().unwrap();
    let before = temporary.path().join("before package");
    let after = temporary.path().join("after package");
    for directory in [&before, &after] {
        package(directory, MODEL);
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
    assert_eq!(std::fs::read_dir(&before).unwrap().count(), 2);
    assert_eq!(
        std::fs::read_to_string(before.join("nested/api.cru")).unwrap(),
        MODEL
    );

    package(&after, &MODEL.replace("/items/", "/v2/items/"));
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

    package(&after, "this is not a model");
    let invalid = diff();
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("does not compile"));
}
