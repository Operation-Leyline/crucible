use crucible_ir::{Audience, ShapeId, ir::Service};
use crucible_lang::{Package, SourceFile, compile};

fn package() -> Package {
    crucible_lang::load_package(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/models/json-schema-unattached"),
    )
    .unwrap()
}
fn lowered(package: &Package) -> Service {
    let compiled = compile(package, None);
    compiled
        .service
        .clone()
        .unwrap_or_else(|| panic!("{}", compiled.render()))
}
#[test]
fn explicit_roots_lower_a_sorted_deterministic_dependency_closure_without_operations() {
    let mut package = package();
    let first = lowered(&package);
    assert!(first.operations.is_empty());
    assert!(first.resources.is_empty());
    assert_eq!(
        first.export_roots,
        [
            ShapeId::new("example.schema", "RenderPlan"),
            ShapeId::new("example.schema", "Secret")
        ]
    );
    assert_eq!(
        first
            .shapes
            .keys()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [
            "crucible.api#Integer",
            "crucible.api#Unit",
            "example.schema#Plans",
            "example.schema#Prompt",
            "example.schema#RenderPlan",
            "example.schema#Secret",
            "example.schema#Seed",
            "example.schema#Source",
        ]
    );
    package.exports = vec!["RenderPlan".into(), "example.schema#Secret".into()];
    let second = lowered(&package);
    assert_eq!(
        crucible_ir::snapshot::write(&first, "test"),
        crucible_ir::snapshot::write(&second, "test")
    );
    let external = crucible_ir::project(&first, Audience::External).unwrap();
    assert_eq!(
        external.export_roots,
        [ShapeId::new("example.schema", "RenderPlan")]
    );
    assert!(
        !external
            .shapes
            .contains_key(&ShapeId::new("example.schema", "Secret"))
    );
    let internal = crucible_ir::project(&first, Audience::Internal).unwrap();
    assert_eq!(internal.export_roots, first.export_roots);
}
#[test]
fn unknown_non_type_and_malformed_roots_are_diagnostics_and_all_are_collected() {
    let mut package = package();
    package.exports = [
        "Missing",
        "Renderer",
        "example.missing#RenderPlan",
        "bad name",
        "example.schema#RenderPlan$seed",
    ]
    .map(str::to_owned)
    .into();
    let compiled = compile(&package, None);
    assert!(compiled.service.is_none());
    let diagnostics = compiled.render();
    for name in &package.exports {
        assert!(diagnostics.contains(name), "{diagnostics}");
    }
    assert!(diagnostics.contains("unknown shape root"), "{diagnostics}");
    assert!(diagnostics.contains("invalid shape root"), "{diagnostics}");
    assert!(diagnostics.contains("not a member"), "{diagnostics}");
}
#[test]
fn ambiguous_imported_names_are_diagnosed_without_picking_a_root() {
    let source = |text: &str| {
        vec![SourceFile {
            path: "model.cru".into(),
            text: text.into(),
        }]
    };
    let mut package = Package::new(
        "example.app",
        source(r#"service App { version "1" protocol json } string Shared"#),
    );
    package.imports = vec!["example.dep".into()];
    package.dependencies = vec![Package::new("example.dep", source("string Shared"))];
    package.exports = vec!["Shared".into()];
    let compiled = compile(&package, None);
    assert!(compiled.service.is_none());
    let diagnostics = compiled.render();
    assert!(
        diagnostics.contains("ambiguous shape root"),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("example.app#Shared") && diagnostics.contains("example.dep#Shared"),
        "{diagnostics}"
    );
}
#[test]
fn external_export_dependencies_use_the_existing_audience_diagnostics() {
    let mut package = Package::new("example.app", vec![SourceFile { path:"model.cru".into(), text:r#"service App { version "1" protocol json } structure Root { value: Secret } internal string Secret"#.into() }]);
    package.exports = vec!["Root".into()];
    let service = lowered(&package);
    let error = crucible_ir::project(&service, Audience::External)
        .unwrap_err()
        .to_string();
    assert!(error.contains("internal"), "{error}");
    assert!(crucible_ir::project(&service, Audience::Internal).is_ok());
}
#[test]
fn snapshots_default_missing_export_roots_and_omit_empty_roots() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/weather");
    let weather = crucible_lang::load_service(&dir).unwrap();
    let text = crucible_ir::snapshot::write(&weather, "test");
    assert!(!text.contains("export_roots"));
    let read = crucible_ir::snapshot::read(&text).unwrap();
    assert!(read.export_roots.is_empty());
    assert_eq!(text, crucible_ir::snapshot::write(&read, "test"));
    let exported = lowered(&package());
    let read =
        crucible_ir::snapshot::read(&crucible_ir::snapshot::write(&exported, "test")).unwrap();
    assert_eq!(read.export_roots, exported.export_roots);
    assert!(
        crucible_ir::project(&read, Audience::External)
            .unwrap()
            .shapes
            .contains_key(&ShapeId::new("example.schema", "RenderPlan"))
    );
}

#[test]
fn imported_types_can_be_explicit_roots_without_an_operation() {
    let source = |text: &str| {
        vec![SourceFile {
            path: "model.cru".into(),
            text: text.into(),
        }]
    };
    let mut package = Package::new(
        "example.app",
        source(r#"service App { version "1" protocol json }"#),
    );
    package.imports = vec!["example.shared".into()];
    package.dependencies = vec![Package::new(
        "example.shared",
        source("string Label { length 1..20 } structure Settings { label: Label }"),
    )];
    package.exports = vec!["example.shared#Settings".into()];
    let service = lowered(&package);
    assert_eq!(
        service.export_roots,
        [ShapeId::new("example.shared", "Settings")]
    );
    assert_eq!(service.shapes.len(), 2);
    assert!(
        service
            .shape(&ShapeId::new("example.shared", "Label"))
            .is_some()
    );
    assert!(
        crucible_ir::project(&service, Audience::External)
            .unwrap()
            .shape(&ShapeId::new("example.shared", "Settings"))
            .is_some()
    );
}

#[test]
fn manifest_root_syntax_errors_point_at_the_export_values() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models/json-schema-unattached");
    let manifest = "[model]\nnamespace = \"example.schema\"\nexports = [\"bad name\", \"example.schema#RenderPlan$seed\"]\n";
    let errors = crucible_lang::check_manifest(&dir, manifest);
    assert_eq!(errors.len(), 2, "{errors:?}");
    for error in errors {
        assert!(
            error.message.contains("invalid shape root"),
            "{}",
            error.message
        );
        let value = &manifest[error.span.start as usize..error.span.end as usize];
        assert!(
            value.contains("bad name") || value.contains("RenderPlan$seed"),
            "{value}"
        );
    }
}

#[test]
fn explicit_exports_survive_client_view_removing_the_only_referring_sender_operation() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/sender-signed");
    let mut package = crucible_lang::load_package(&dir).unwrap();
    package.exports = vec!["LeakedKey".into()];
    let service = lowered(&package);
    let client = crucible_ir::client_view(&service);
    assert!(
        client
            .operations
            .iter()
            .all(|operation| operation.sender.is_none())
    );
    let id = ShapeId::new("example.hooks", "LeakedKey");
    assert_eq!(client.export_roots.as_slice(), std::slice::from_ref(&id));
    assert!(client.shape(&id).is_some());
}

#[test]
fn export_diagnostics_are_collected_even_when_service_selection_fails() {
    let mut package = package();
    package.exports = vec!["Missing".into()];
    let compiled = compile(&package, Some("MissingService"));
    assert!(compiled.service.is_none());
    let diagnostics = compiled.render();
    assert!(diagnostics.contains("unknown shape root"), "{diagnostics}");
    assert!(diagnostics.contains("has no service"), "{diagnostics}");
}
