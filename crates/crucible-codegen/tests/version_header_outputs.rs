#![cfg(all(feature = "docs", feature = "rust"))]
mod common;
#[test]
fn sdk_http_guide_and_snippets_describe_the_header_and_policy() {
    let service = common::load("version-header-named");
    let file = crucible_codegen::sdk::SdkFile {
        path: "crucible-sdk.toml".into(),
        model: "model".into(),
        service: None,
        sdks: Default::default(),
        docs: None,
    };
    let document = crucible_codegen::sdks::document(&file, &service, &[]).unwrap();
    let text = crucible_codegen::sdks::render(&document);
    assert!(text.contains("Example-Version"));
    assert!(text.contains("HTTP 400"));
    assert!(!text.contains("Crucible-Version"));
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/version-header.http.json");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
}

#[cfg(feature = "json-schema")]
#[test]
fn value_schemas_accept_the_release_settings() {
    let service = common::load("version-header-named");
    let options = crucible_codegen_jsonschema::Options {
        shape: "PingOutput".into(),
    };
    assert!(
        !crucible_codegen_jsonschema::generate(&service, &options)
            .unwrap()
            .is_empty()
    );
}
