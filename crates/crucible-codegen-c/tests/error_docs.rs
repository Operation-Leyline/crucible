use crucible_codegen_c::{Options, emit};

fn service() -> crucible_ir::ir::Service {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/error-docs");
    let full = crucible_lang::load_service(&path).unwrap();
    crucible_ir::project(&full, crucible_ir::Audience::External).unwrap()
}
fn options() -> Options {
    Options {
        package_name: "docs_client".into(),
        prefix: "docs".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
        metadata: Default::default(),
    }
}
#[test]
fn configured_documentation_uses_projected_static_schema() {
    let files = emit(&service(), &options()).unwrap();
    let output = files
        .iter()
        .map(|file| file.contents.as_str())
        .collect::<String>();
    assert!(output.contains("reasonDefault"));
    assert!(output.contains("reason-access%2Fneeded"));
    assert!(!output.contains("reason-secret"));
    assert!(output.contains("doc_url"));
}
#[test]
fn no_base_runtime_is_unchanged() {
    let mut service = service();
    service.error_docs_base = None;
    let files = emit(&service, &options()).unwrap();
    let output = files
        .iter()
        .map(|file| file.contents.as_str())
        .collect::<String>();
    assert!(!output.contains("reasonUrls"));
    assert!(!output.contains("doc_url"));
}

#[test]
fn no_base_runtime_matches_original_templates_byte_for_byte() {
    let mut service = service();
    service.error_docs_base = None;
    let files = emit(&service, &options()).unwrap();
    let header = files
        .iter()
        .find(|f| f.path == "include/docs_client/runtime/client_runtime.h")
        .unwrap();
    assert_eq!(
        header.contents,
        include_str!("../runtime/client_runtime.h")
            .replace("crucible_generated", "docs")
            .replace("CRUCIBLE_GENERATED", "DOCS")
    );
    let bindings = files.iter().find(|f| f.path == "src/bindings.c").unwrap();
    let mut expected = include_str!("../runtime/bindings.c")
        .replace("crucible_generated", "docs")
        .replace("CRUCIBLE_GENERATED", "DOCS");
    for header in ["types.h", "schema.h", "http.h", "client_runtime.h"] {
        expected = expected.replace(
            &format!("\"{header}\""),
            &format!("\"docs_client/runtime/{header}\""),
        );
    }
    assert_eq!(bindings.contents, expected);
}
