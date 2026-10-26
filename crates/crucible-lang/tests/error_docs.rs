use crucible_ir::{Audience, ShapeId, ir::Shape, project, snapshot};
use std::path::Path;

fn load() -> crucible_ir::ir::Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/error-docs");
    crucible_lang::load_service(&dir).unwrap_or_else(|error| panic!("{error}"))
}
fn id(name: &str) -> ShapeId {
    ShapeId::new("example.error_docs", name)
}

#[test]
fn error_documentation_syntax_loads_as_a_package() {
    let service = load();
    assert_eq!(
        service.error_docs_base.as_deref(),
        Some("https://docs.example.test/errors")
    );
    for (name, member) in [
        ("Denied", "detail"),
        ("HeaderError", "reason"),
        ("PayloadError", "reason"),
    ] {
        let docs = service.error_documentation(&id(name)).unwrap();
        assert_eq!(docs.reason_member.as_deref(), Some(member));
        assert_eq!(docs.reasons.len(), 5);
        assert!(docs.reasons.iter().any(|r| r.url.as_deref()
            == Some(&format!(
                "https://docs.example.test/errors/{name}/reason-access%2Fneeded"
            ))));
        assert!(docs.reasons.iter().any(|r| r.url.as_deref()
            == Some(&format!(
                "https://docs.example.test/errors/{name}/reason-%2E%2E"
            ))));
    }
    assert!(
        service
            .error_documentation(&id("PlainError"))
            .unwrap()
            .reasons
            .is_empty()
    );
}

#[test]
fn refusal_package_collects_every_invalid_declaration() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/error-docs-invalid");
    let error = crucible_lang::load_service(&dir).unwrap_err();
    for expected in [
        "applies to members only",
        "no credentials",
        "one `errorDocs`",
        "error only",
        "string enum",
        "takes no arguments",
        "at most one",
    ] {
        assert!(error.contains(expected), "missing {expected}: {error}");
    }
}

#[test]
fn external_projection_filters_internal_reason_values_and_members() {
    let mut service = load();
    let projected = project(&service, Audience::External).unwrap();
    let docs = projected.error_documentation(&id("Denied")).unwrap();
    assert_eq!(docs.reasons.len(), 4);
    assert!(!docs.reasons.iter().any(|r| r.value == "secret"));
    let Shape::Structure(error) = service.shapes.get_mut(&id("Denied")).unwrap() else {
        panic!()
    };
    error
        .members
        .iter_mut()
        .find(|m| m.name == "detail")
        .unwrap()
        .internal = true;
    let projected = project(&service, Audience::External).unwrap();
    let docs = projected.error_documentation(&id("Denied")).unwrap();
    assert_eq!(docs.reason_member, None);
    assert!(docs.reasons.is_empty());
}

#[test]
fn snapshots_default_metadata_and_wire_contracts_ignore_documentation() {
    let service = load();
    let text = snapshot::write(&service, "test");
    let restored = snapshot::read(&text).unwrap();
    assert_eq!(
        restored.error_documentation(&id("Denied")),
        service.error_documentation(&id("Denied"))
    );
    let mut old: serde_json::Value = serde_json::from_str(&text).unwrap();
    old["service"]
        .as_object_mut()
        .unwrap()
        .remove("error_docs_base");
    for shape in old["service"]["shapes"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        if let Some(error) = shape
            .get_mut("Structure")
            .and_then(|s| s.get_mut("error"))
            .and_then(|e| e.as_object_mut())
        {
            error.remove("reason_member");
        }
    }
    let old = snapshot::read(&old.to_string()).unwrap();
    assert_eq!(old.error_docs_base, None);
    assert_eq!(
        old.error_documentation(&id("Denied"))
            .unwrap()
            .reason_member,
        None
    );
    assert_eq!(
        snapshot::write_contract(&old, "test"),
        snapshot::write_contract(&service, "test")
    );
}

#[test]
fn manually_lowered_invalid_documentation_is_diagnosed() {
    let mut service = load();
    service.error_docs_base = Some("javascript:bad".into());
    let Shape::Structure(error) = service.shapes.get_mut(&id("Denied")).unwrap() else {
        panic!()
    };
    let info = error.error.as_mut().unwrap();
    info.wire_code = "..".into();
    info.reason_member = Some("missing".into());
    let mut diagnostics = crucible_ir::Diagnostics::default();
    crucible_ir::validate::service(&service, &mut diagnostics);
    let error = diagnostics.to_string();
    for expected in ["absolute HTTP", "dot segment", "string-enum member"] {
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn targets_must_explicitly_support_configured_error_documentation() {
    let mut service = load();
    let capabilities = crucible_ir::Capabilities::new(
        crucible_ir::Feature::ALL
            .iter()
            .copied()
            .filter(|feature| *feature != crucible_ir::Feature::ErrorDocumentation),
    );
    assert!(
        service
            .require_capabilities(&capabilities)
            .unwrap_err()
            .to_string()
            .contains("unsupported target capability error-documentation")
    );
    service.error_docs_base = None;
    service.require_capabilities(&capabilities).unwrap();
}

#[test]
fn direct_ir_bases_derive_the_same_urls_as_compiler_normalized_bases() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/error-docs");
    for base in [
        "https://docs.example.test/errors/",
        "https://docs.example.test",
        "http://localhost:8000",
        "https://docs.example.test/a/../errors/",
    ] {
        let mut package = crucible_lang::load_package(&dir).unwrap();
        for file in &mut package.files {
            file.text = file.text.replace("https://docs.example.test/errors/", base);
        }
        let compiled = crucible_lang::compile(&package, None);
        let rendered = compiled.render();
        let canonical = compiled.service.unwrap_or_else(|| panic!("{rendered}"));
        let mut direct = canonical.clone();
        direct.error_docs_base = Some(base.into());
        assert_eq!(
            direct.error_documentation(&id("Denied")),
            canonical.error_documentation(&id("Denied")),
            "{base}"
        );
    }
}
