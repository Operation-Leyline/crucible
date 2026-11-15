//! Clearable optional input members retain explicit clearing intent in the IR.
use std::path::{Path, PathBuf};

use crucible_ir::{Capabilities, Feature, ir::Shape};
use crucible_lang::{FormatOptions, format};

fn package(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn refuses(name: &str, message: &str) {
    let error = crucible_lang::load_service(&package(name)).unwrap_err();
    assert!(error.contains(message), "{error}");
}

#[test]
fn optional_input_and_elided_property_lower_with_the_flag() {
    let service = crucible_lang::load_service(&package("clearable")).unwrap();
    for operation in &service.operations {
        let Shape::Structure(input) = service.shape(operation.input.as_ref().unwrap()).unwrap()
        else {
            panic!("input is a structure");
        };
        let member = input
            .members
            .iter()
            .find(|m| m.name == "description")
            .unwrap();
        assert!(member.clearable);
        assert!(member.optional_on_server());
        assert!(
            input
                .members
                .iter()
                .filter(|m| m.name != "description")
                .all(|m| !m.clearable)
        );
    }
    let uses: Vec<_> = service
        .requirements
        .iter()
        .filter(|r| r.feature == Feature::Clearable)
        .collect();
    assert_eq!(uses.len(), 2);
    let error = service
        .require_capabilities(&Capabilities::default())
        .unwrap_err();
    assert!(format!("{error:?}").contains("unsupported target capability clearable"));
}

#[test]
fn required_member_is_refused() {
    refuses(
        "clearable-required",
        "`clearable` requires an optional (`?`) member",
    );
}
#[test]
fn defaulted_member_is_refused() {
    refuses(
        "clearable-defaulted",
        "`clearable` does not apply to a defaulted member",
    );
}
#[test]
fn output_member_is_refused() {
    refuses(
        "clearable-output",
        "`clearable` does not apply to an output member",
    );
}
#[test]
fn error_member_is_refused() {
    refuses(
        "clearable-error",
        "`clearable` does not apply to an error member",
    );
}
#[test]
fn header_member_is_refused() {
    refuses(
        "clearable-header",
        "`clearable` does not apply to a header member",
    );
}
#[test]
fn query_member_is_refused() {
    refuses(
        "clearable-query",
        "`clearable` does not apply to a query member",
    );
}
#[test]
fn path_member_is_refused() {
    refuses(
        "clearable-path",
        "`clearable` does not apply to a path member",
    );
}
#[test]
fn union_arm_is_refused() {
    refuses(
        "clearable-union",
        "`clearable` does not apply to a union arm",
    );
}
#[test]
fn nested_member_is_refused() {
    refuses(
        "clearable-nested",
        "`clearable` applies only to an operation input member",
    );
}
#[test]
fn include_directive_is_refused() {
    refuses(
        "clearable-include",
        "`clearable` marks an optional member, not an `include` directive",
    );
}
#[test]
fn formatter_round_trips_the_modifier() {
    let source = "operation UpdateNote {\n    input {\n        sensitive clearable description?: string\n    }\n}\n";
    assert_eq!(format(source, &FormatOptions::default()), source);
}

#[test]
fn parser_retains_the_member_modifier() {
    use crucible_lang::ast::{Decl, OpEntry};
    for member in ["description?: string", "$description?"] {
        let source = format!("operation Update {{ input {{ clearable {member} }} }}");
        let parsed = crucible_lang::parse(&source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let Decl::Operation(operation) = &parsed.file.items[0].decl else {
            panic!("operation")
        };
        let OpEntry::Body(body) = &operation.entries[0] else {
            panic!("body")
        };
        assert_eq!(body.members[0].modifiers[0].name, "clearable");
    }
}
