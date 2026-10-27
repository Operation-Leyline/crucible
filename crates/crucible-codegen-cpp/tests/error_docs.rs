use crucible_codegen_cpp::{Options, emit};

fn service() -> crucible_ir::ir::Service {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/error-docs");
    let full = crucible_lang::load_service(&path).unwrap();
    crucible_ir::project(&full, crucible_ir::Audience::External).unwrap()
}
fn options() -> Options {
    Options {
        package_name: "docs_client".into(),
        namespace: "docs".into(),
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
    for (path, source) in [
        (
            "include/docs_client/runtime/types.hpp",
            include_str!("../runtime/types.hpp"),
        ),
        (
            "src/client_runtime.cpp",
            include_str!("../runtime/client_runtime.cpp"),
        ),
    ] {
        let mut expected = crucible_codegen_common::service_exception_runtime(&service, source)
            .replace("crucible_generated", "docs");
        if path.starts_with("src/") {
            for header in [
                "json.hpp",
                "timestamp.hpp",
                "types.hpp",
                "codec.hpp",
                "transport.hpp",
                "client_runtime.hpp",
            ] {
                expected = expected.replace(
                    &format!("\"{header}\""),
                    &format!("\"docs_client/runtime/{header}\""),
                );
            }
        }
        assert_eq!(
            files.iter().find(|f| f.path == path).unwrap().contents,
            expected
        );
    }
}
#[test]
fn configured_accessor_collisions_are_diagnosed() {
    let mut service = service();
    for shape in service.shapes.values_mut() {
        if let crucible_ir::ir::Shape::Structure(structure) = shape
            && structure.error.is_some()
        {
            let mut member = structure.members[0].clone();
            member.name = "docUrl".into();
            structure.members.push(member);
        }
    }
    assert!(emit(&service, &options()).is_err());
    service.error_docs_base = None;
    assert!(emit(&service, &options()).is_ok());
}
