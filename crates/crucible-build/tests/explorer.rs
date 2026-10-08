#![cfg(feature = "rust")]
use crucible_build::{Audience, model};

fn fixture() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crucible-smithy/tests/fixtures/service-docs-weather.json")
}

#[test]
fn explorer_is_opt_in_and_audience_selection_is_order_independent() {
    let source = fixture();
    assert!(
        !model(&source)
            .server()
            .render()
            .unwrap()
            .contains("static DESCRIPTION")
    );
    let internal = model(&source).server().explorer().render().unwrap();
    assert!(internal.contains("static DESCRIPTION"));
    assert!(internal.contains("describe/1"));
    assert!(internal.contains("example.weather#Status"));
    let a = model(&source)
        .server()
        .explorer()
        .audience(Audience::External)
        .render()
        .unwrap();
    let b = model(&source)
        .server()
        .audience(Audience::External)
        .explorer()
        .render()
        .unwrap();
    assert_eq!(a, b);
    let description = a.split("static DESCRIPTION").nth(1).unwrap();
    assert!(!description.contains("example.weather#Status"));
    // Normal server registration always keeps internal operations.
    assert!(
        a.split("static DESCRIPTION")
            .next()
            .unwrap()
            .contains("StatusOperation")
    );
}

#[test]
fn explorer_generated_bindings_are_valid_rust() {
    let code = model(fixture()).server().explorer().render().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("service.rs");
    std::fs::write(&file, code).unwrap();
    assert!(
        std::process::Command::new("rustfmt")
            .arg("--edition=2024")
            .arg(file)
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn explorer_preserves_normal_server_bindings_and_resource_documentation() {
    let mut source: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture()).unwrap()).unwrap();
    source["shapes"]["example.weather#ResourceOnlyId"] = serde_json::json!({"type": "string"});
    source["shapes"]["example.weather#City"]["identifiers"]["tracking"] =
        serde_json::json!({"target": "example.weather#ResourceOnlyId"});
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.json");
    std::fs::write(&path, source.to_string()).unwrap();
    let plain = model(&path).server().render().unwrap();
    let explorer = model(&path).server().explorer().render().unwrap();
    let prefix = explorer
        .split("    /// Embedded Explorer contract")
        .next()
        .unwrap();
    assert_eq!(
        plain.strip_suffix("}\n").unwrap().trim_end(),
        prefix.trim_end()
    );
    assert!(explorer.contains("example.weather#City"));
    assert!(!plain.contains("ResourceOnlyId"));
    assert!(explorer.contains("ResourceOnlyId"));
}
