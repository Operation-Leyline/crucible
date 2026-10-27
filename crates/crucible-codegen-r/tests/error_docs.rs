use crucible_codegen_r::{Options, emit};

fn service() -> crucible_ir::ir::Service {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/error-docs");
    let full = crucible_lang::load_service(&path).unwrap();
    crucible_ir::project(&full, crucible_ir::Audience::External).unwrap()
}
fn options() -> Options {
    Options {
        package_name: "docs.client".into(),
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
fn no_base_runtime_matches_original_template_byte_for_byte() {
    let mut service = service();
    service.error_docs_base = None;
    let files = emit(&service, &options()).unwrap();
    let runtime = files.iter().find(|f| f.path == "R/runtime.R").unwrap();
    assert_eq!(
        runtime.contents,
        crucible_codegen_common::service_exception_runtime(
            &service,
            include_str!("../runtime/runtime.R")
        )
    );
}
