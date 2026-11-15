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
fn raw_payload_members_are_refused() {
    let error = crucible_lang::load_service(&package("clearable-raw-payload")).unwrap_err();
    assert_eq!(
        error
            .matches("`clearable` does not apply to a raw `@payload` member")
            .count(),
        3,
        "{error}",
    );
}

#[test]
fn json_payload_members_remain_clearable() {
    let service = crucible_lang::load_service(&package("clearable-json-payload")).unwrap();
    for operation in &service.operations {
        assert_eq!(
            operation.request.payload.as_ref().unwrap().kind,
            crucible_ir::ir::PayloadKind::Document
        );
        let Shape::Structure(input) = service.shape(operation.input.as_ref().unwrap()).unwrap()
        else {
            panic!("input is a structure");
        };
        assert!(input.members[0].clearable);
    }
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

fn null_input_example(modifier: &str) -> crucible_lang::Compiled {
    let text = format!(
        r#"
service Notes {{ version "1" protocol json operations [Update] }}
operation Update {{
    http PATCH "/notes"
    input {{ {modifier} description?: string }}
    example "Clear description" {{ input {{ description: null }} }}
}}
"#
    );
    crucible_lang::compile(
        &crucible_lang::Package::new(
            "example.clearable",
            vec![crucible_lang::SourceFile {
                path: "notes.cru".into(),
                text,
            }],
        ),
        None,
    )
}

#[test]
fn clearable_null_input_example_compiles() {
    let compiled = null_input_example("clearable");
    assert!(!compiled.has_errors(), "{}", compiled.render());
    let service = compiled.service.unwrap();
    assert_eq!(
        service.operations[0].examples[0]
            .input
            .as_ref()
            .unwrap()
            .get("description"),
        Some(&serde_json::Value::Null)
    );
}

#[test]
fn ordinary_optional_null_input_example_is_refused() {
    let compiled = null_input_example("");
    assert!(compiled.has_errors());
    assert!(
        compiled
            .render()
            .contains("input.description must not be null"),
        "{}",
        compiled.render()
    );
}

#[test]
fn metadata_modifier_names_are_not_reserved() {
    for name in ["sensitive", "internal", "clearable"] {
        let text = format!(
            r#"
metadata {name} on member
service Notes {{ version "1" protocol json operations [Update] }}
operation Update {{
    http PATCH "/notes"
    input {{ @{name} description?: string }}
}}
"#
        );
        let compiled = crucible_lang::compile(
            &crucible_lang::Package::new(
                "example.clearable",
                vec![crucible_lang::SourceFile {
                    path: "notes.cru".into(),
                    text,
                }],
            ),
            None,
        );
        assert!(!compiled.has_errors(), "{}", compiled.render());
        let service = compiled.service.unwrap();
        let Shape::Structure(input) = service
            .shape(service.operations[0].input.as_ref().unwrap())
            .unwrap()
        else {
            panic!("input structure");
        };
        let member = input.member("description").unwrap();
        assert!(!member.internal && !member.sensitive && !member.clearable);
        assert!(
            member
                .traits
                .contains_key(&crucible_ir::ShapeId::new("example.clearable", name))
        );
    }
}

#[test]
fn metadata_conflicts_can_name_the_clearable_modifier() {
    let package = crucible_lang::Package::new(
        "example.clearable",
        vec![crucible_lang::SourceFile {
            path: "notes.cru".into(),
            text: r#"
metadata secret on member conflicts clearable
service Notes { version "1" protocol json operations [Update] }
operation Update {
    http PATCH "/notes"
    input { @secret clearable description?: string }
}
"#
            .into(),
        }],
    );
    let compiled = crucible_lang::compile(&package, None);
    assert!(compiled.has_errors());
    assert!(
        compiled
            .render()
            .contains("`@secret` conflicts with `clearable`"),
        "{}",
        compiled.render()
    );
}
