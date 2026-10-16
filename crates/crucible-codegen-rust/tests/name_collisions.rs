//! A valid model whose Rust spelling collides is diagnosed, or generated
//! without the collision, never emitted as uncompilable code.
use crucible_codegen_rust::{Options, generate, generate_client, pages_method};
use crucible_ir::ShapeId;
use crucible_lang::SourceFile;

mod common;

fn weather(edit: impl FnOnce(&mut crucible_lang::Package)) -> crucible_ir::ir::Service {
    common::load_with("weather", edit)
}

fn add_source(package: &mut crucible_lang::Package, text: &str) {
    package.files.push(SourceFile {
        path: "added.cru".into(),
        text: text.into(),
    });
}

/// Adds `source` and a `reached` member of `CitySummary` that targets `target`.
fn reach(package: &mut crucible_lang::Package, source: &str, target: &str) {
    add_source(package, source);
    for file in &mut package.files {
        file.text = file.text.replace(
            "structure CitySummary {\n",
            &format!("structure CitySummary {{\n    reached?: {target}\n"),
        );
    }
}

fn both_diagnose(service: &crucible_ir::ir::Service, expected: &str) {
    for diagnostics in [
        generate(service, &Options::default())
            .unwrap_err()
            .to_string(),
        generate_client(service).unwrap_err().to_string(),
    ] {
        assert!(diagnostics.contains(expected), "{diagnostics}");
    }
}

#[test]
fn members_colliding_after_case_conversion_are_diagnosed() {
    let service = weather(|package| {
        for file in &mut package.files {
            file.text = file.text.replace(
                "structure CitySummary {\n",
                "structure CitySummary {\n    fooBar?: string\n    foo_bar?: string\n",
            );
        }
    });
    both_diagnose(
        &service,
        "example.weather#CitySummary$foo_bar: members `fooBar` and `foo_bar` are both field `foo_bar` in Rust",
    );
}

#[test]
fn error_members_colliding_after_case_conversion_are_diagnosed() {
    let service = weather(|package| {
        for file in &mut package.files {
            file.text = file.text.replace(
                "error NotFoundError {\n",
                "error NotFoundError {\n    fooBar?: string\n    foo_bar?: string\n",
            );
        }
    });
    both_diagnose(
        &service,
        "example.weather#NotFoundError$foo_bar: members `fooBar` and `foo_bar` are both field `foo_bar` in Rust",
    );
}

#[test]
fn int_enum_variants_colliding_after_case_conversion_are_diagnosed() {
    let service = weather(|package| {
        reach(
            package,
            "enum Level: integer {\n    fooBar = 1\n    FOO_BAR = 2\n}\n",
            "Level",
        );
    });
    // Which member is reported depends on the order the model keeps.
    both_diagnose(&service, "are both variant `FooBar` in Rust");
}

#[test]
fn union_variants_colliding_after_case_conversion_are_diagnosed() {
    let service = weather(|package| {
        reach(
            package,
            "union Choice {\n    fooBar: string\n    FOO_BAR: string\n}\n",
            "Choice",
        );
    });
    both_diagnose(&service, "are both variant `FooBar` in Rust");
}

/// `ListCities` is paginated and `ListCitiesPages` is another operation: the
/// operation keeps `list_cities_pages`, the paginator takes a suffix.
#[test]
fn a_paginator_does_not_take_another_operations_method_name() {
    let service = weather(|package| {
        add_source(
            package,
            "readonly operation ListCitiesPages {\n    http GET \"/list-cities-pages\"\n}\n",
        );
        for file in &mut package.files {
            file.text = file.text.replace(
                "operations [Ping, Status]",
                "operations [Ping, Status, ListCitiesPages]",
            );
        }
    });
    let code = generate_client(&service).unwrap();
    for method in [
        "pub async fn list_cities_pages(",
        "pub fn list_cities_pages_2(",
    ] {
        assert_eq!(code.matches(method).count(), 1, "{method}\n{code}");
    }
    assert!(!code.contains("pub fn list_cities_pages("), "{code}");
    let list = service
        .operations
        .iter()
        .find(|op| op.id == ShapeId::new("example.weather", "ListCities"))
        .unwrap();
    // The generated service CLI calls the method the SDK names.
    assert_eq!(
        pages_method(&service, list).as_deref(),
        Some("list_cities_pages_2")
    );
}
