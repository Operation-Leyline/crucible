use std::path::Path;
#[test]
fn configured_error_documentation_is_supported() {
    let service = crucible_lang::load_service(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/error-docs"),
    )
    .unwrap();
    let service = crucible_ir::project(&service, crucible_ir::Audience::External).unwrap();
    let options = crucible_codegen_dart::Options {
        metadata: Default::default(),
        package_name: "docs_client".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
    };
    let files =
        crucible_codegen_dart::emit(&service, &options).unwrap_or_else(|error| panic!("{error:?}"));
    let models = &files
        .iter()
        .find(|file| file.path.ends_with("models.dart"))
        .unwrap()
        .contents;
    assert!(models.contains("docUrl"));
    assert!(models.contains("reason-access%2Fneeded"));
    assert!(!models.contains("reason-secret"));
    let runtime = &files
        .iter()
        .find(|file| file.path.ends_with("runtime/errors.dart"))
        .unwrap()
        .contents;
    assert!(runtime.contains("docUrl"));
    let mut collision = service.clone();
    let shape = collision
        .shapes
        .iter_mut()
        .find_map(|(_, shape)| match shape {
            crucible_ir::ir::Shape::Structure(shape)
                if shape.error.is_some() && !shape.members.is_empty() =>
            {
                Some(shape)
            }
            _ => None,
        })
        .unwrap();
    shape.members[0].name = "docUrl".into();
    let error = crucible_codegen_dart::emit(&collision, &options)
        .err()
        .expect("configured accessor collision");
    assert!(format!("{error:?}").contains("configured"));
    collision.error_docs_base = None;
    crucible_codegen_dart::emit(&collision, &options)
        .expect("legacy modeled URL member remains supported without a base");
    let mut no_base = service.clone();
    no_base.error_docs_base = None;
    let legacy = crucible_codegen_dart::emit(&no_base, &options).unwrap();
    assert!(
        !legacy
            .iter()
            .find(|file| file.path.ends_with("runtime/errors.dart"))
            .unwrap()
            .contents
            .contains("docUrl")
    );
    let golden =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/error-documentation.dart");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        std::fs::write(&golden, models).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(golden)
            .unwrap()
            .replace("\r\n", "\n"),
        models.replace("\r\n", "\n")
    );
}
