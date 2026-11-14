use crucible_codegen_common::{
    describe, guide, snippet,
    surface::{ClientType, Package, Surface},
};
use std::path::Path;
fn service() -> crucible_ir::ir::Service {
    crucible_lang::load_service(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/version-header-named"),
    )
    .unwrap()
}
fn golden(name: &str, text: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
}
#[test]
fn describe_names_the_header_and_policy() {
    let d = describe::describe(&service()).unwrap();
    assert_eq!(d.versions["carrier"]["name"], "Example-Version");
    assert_eq!(d.versions["unversioned"], "required");
    golden("version-header.describe.json", &d.to_json());
}
#[test]
fn request_syntax_names_the_configured_header() {
    let headers = snippet::added_headers(&service(), &[]).join("\n") + "\n";
    assert_eq!(headers, "Example-Version: v1\n");
    golden("version-header.snippets.txt", &headers);
}
#[test]
fn sdk_guides_name_the_configured_header_and_policy() {
    let surface = Surface::new(
        "example",
        Package::new("example", "example-sdk", "example_sdk"),
        ClientType::new("ExampleClient", ""),
    );
    let text = guide::readme(&service(), &surface, &guide::Guide::new("Example", "text"));
    assert!(text.contains("Example-Version: v1"));
    assert!(text.contains("HTTP 400"));
    golden("version-header.guide.md", &text);
}

#[test]
fn agent_guides_name_the_configured_header_and_policy() {
    let surface = Surface::new(
        "example",
        Package::new("example", "example-sdk", "example_sdk"),
        ClientType::new("ExampleClient", ""),
    );
    let mut guide = guide::Guide::new("Example", "text");
    guide.auth = "This service takes no credential.".into();
    let text = guide::agents(&service(), &surface, &guide);
    assert!(text.contains("Example-Version: v1"));
    assert!(text.contains("HTTP 400"));
    golden("version-header.agents.md", &text);
}

#[test]
fn native_schema_carries_the_declared_header_spelling() {
    let schema = crucible_codegen_common::schema(&service(), str::to_owned);
    assert_eq!(schema["apiVersionHeader"], "Example-Version");
    assert_eq!(schema["apiVersion"], "v1");
}

#[test]
fn native_schema_omits_the_default_header() {
    let mut service = service();
    service.version_header = None;
    assert!(
        crucible_codegen_common::schema(&service, str::to_owned)
            .get("apiVersionHeader")
            .is_none()
    );
}

#[test]
fn native_schema_omits_a_case_only_default_spelling() {
    let mut service = service();
    service.version_header = Some("CRUCIBLE-version".into());
    assert!(
        crucible_codegen_common::schema(&service, str::to_owned)
            .get("apiVersionHeader")
            .is_none()
    );
}
