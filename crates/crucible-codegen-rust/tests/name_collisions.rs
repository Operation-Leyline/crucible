//! Valid Smithy whose Rust spelling collides is diagnosed, or generated
//! without the collision, never emitted as uncompilable code.
use crucible_codegen_rust::{Options, generate, generate_client, pages_method};
use crucible_smithy::{Model, ShapeId};
use serde_json::{Value, json};

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

fn weather(edit: impl FnOnce(&mut Value)) -> crucible_smithy::ir::Service {
    let mut ast: Value = serde_json::from_str(WEATHER).unwrap();
    edit(&mut ast);
    Model::from_json(&ast.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_rust::capabilities())
        .unwrap()
}

fn reach(ast: &mut Value, id: &str, shape: Value) {
    ast["shapes"][id] = shape;
    ast["shapes"]["example.weather#CitySummary"]["members"]["reached"] = json!({ "target": id });
}

fn both_diagnose(service: &crucible_smithy::ir::Service, expected: &str) {
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
    let service = weather(|ast| {
        let members = &mut ast["shapes"]["example.weather#CitySummary"]["members"];
        members["fooBar"] = json!({ "target": "smithy.api#String" });
        members["foo_bar"] = json!({ "target": "smithy.api#String" });
    });
    both_diagnose(
        &service,
        "example.weather#CitySummary$foo_bar: members `fooBar` and `foo_bar` are both field `foo_bar` in Rust",
    );
}

#[test]
fn error_members_colliding_after_case_conversion_are_diagnosed() {
    let service = weather(|ast| {
        let members = &mut ast["shapes"]["example.weather#NotFoundError"]["members"];
        members["fooBar"] = json!({ "target": "smithy.api#String" });
        members["foo_bar"] = json!({ "target": "smithy.api#String" });
    });
    both_diagnose(
        &service,
        "example.weather#NotFoundError$foo_bar: members `fooBar` and `foo_bar` are both field `foo_bar` in Rust",
    );
}

#[test]
fn int_enum_variants_colliding_after_case_conversion_are_diagnosed() {
    let service = weather(|ast| {
        reach(
            ast,
            "example.weather#Level",
            json!({ "type": "intEnum", "members": {
                "fooBar": { "target": "smithy.api#Unit", "traits": { "smithy.api#enumValue": 1 } },
                "FOO_BAR": { "target": "smithy.api#Unit", "traits": { "smithy.api#enumValue": 2 } },
            } }),
        );
    });
    // Which member is reported depends on whether serde_json keeps source order.
    both_diagnose(&service, "are both variant `FooBar` in Rust");
}

#[test]
fn union_variants_colliding_after_case_conversion_are_diagnosed() {
    let service = weather(|ast| {
        reach(
            ast,
            "example.weather#Choice",
            json!({ "type": "union", "members": {
                "fooBar": { "target": "smithy.api#String" },
                "FOO_BAR": { "target": "smithy.api#String" },
            } }),
        );
    });
    both_diagnose(&service, "are both variant `FooBar` in Rust");
}

/// `ListCities` is paginated and `ListCitiesPages` is another operation: the
/// operation keeps `list_cities_pages`, the paginator takes a suffix.
#[test]
fn a_paginator_does_not_take_another_operations_method_name() {
    let service = weather(|ast| {
        ast["shapes"]["example.weather#ListCitiesPages"] =
            ast["shapes"]["example.weather#Ping"].clone();
        ast["shapes"]["example.weather#ListCitiesPages"]["traits"]["smithy.api#http"]["uri"] =
            json!("/list-cities-pages");
        ast["shapes"]["example.weather#Weather"]["operations"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "target": "example.weather#ListCitiesPages" }));
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
