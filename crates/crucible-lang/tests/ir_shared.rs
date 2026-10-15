//! Rules the IR itself implements (pattern translation, wire examples), run on
//! lowered `.cru` test packages.

use std::path::Path;

use crucible_smithy::ir::{Service, Shape, TimestampFormat};
use serde_json::json;

fn load(name: &str) -> Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&dir).unwrap_or_else(|error| panic!("{error}"))
}

fn check_input(service: &Service) -> &crucible_smithy::ir::Structure {
    let operation = service
        .operations
        .iter()
        .find(|op| op.name() == "Check")
        .unwrap();
    let Some(Shape::Structure(input)) = service.shape(operation.input.as_ref().unwrap()) else {
        panic!("input structure")
    };
    input
}

fn pattern(input: &crucible_smithy::ir::Structure, member: &str) -> String {
    input
        .member(member)
        .unwrap()
        .constraints
        .rust_pattern()
        .unwrap()
        .unwrap()
}

#[test]
fn ecma_patterns_translate_from_effective_ir_constraints() {
    let service = load("patterns");
    let input = check_input(&service);
    for member in &input.members {
        assert!(
            member.constraints.rust_pattern().unwrap().is_some(),
            "{}",
            member.name
        );
    }
    assert_eq!(pattern(input, "digits"), "^[0-9]+$");
    assert_eq!(pattern(input, "literal"), r"^\\d\.\x{00e9}\x{41}\x{01}$");
    for (member, translated) in [
        ("empty", r"^[^\x{0}-\x{10FFFF}]$"),
        ("any", r"^[\x{0}-\x{10FFFF}]$"),
        ("anySuffix", r"^[\x{0}-\x{10FFFF}]x\]$"),
    ] {
        assert_eq!(pattern(input, member), translated);
    }
}

#[test]
fn long_operation_names_are_valid_models() {
    let service = load("mcp-tool-names");
    assert_eq!(service.operations.len(), 2);
    assert!(service.operations.iter().all(|op| op.name().len() > 128));
}

#[test]
fn wire_examples_use_json_names_and_the_members_timestamp_format() {
    let service = load("weather");
    let ping = service.operations.iter().find(|op| op.name() == "Ping");
    assert!(ping.unwrap().examples.is_empty());
    let create = service
        .operations
        .iter()
        .find(|op| op.name() == "CreateCity")
        .unwrap();
    let Some(Shape::Structure(output)) = service.shape(create.output.as_ref().unwrap()) else {
        panic!("output structure")
    };
    let city = output.member("city").unwrap();
    let value = &create.examples[0].output.as_ref().unwrap()["city"];
    let wire = service.wire_example(city, value, None);
    // `createdAt` is epoch seconds by its own format; `updatedAt` follows the protocol.
    assert_eq!(wire["createdAt"], json!(1767323045));
    assert_eq!(wire["updatedAt"], json!("2026-01-02T03:04:05Z"));
    let http = service.wire_example(
        city,
        &json!({"updatedAt": "2026-01-02T03:04:05Z"}),
        Some(TimestampFormat::EpochSeconds),
    );
    // A binding's format applies to the bound member, never inside a document.
    assert_eq!(http["updatedAt"], json!("2026-01-02T03:04:05Z"));
}
