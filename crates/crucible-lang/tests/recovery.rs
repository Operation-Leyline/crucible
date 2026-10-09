//! Parsing reports every problem and keeps going.

use crucible_lang::ast::Decl;
use crucible_lang::parse;

fn names(source: &str) -> (Vec<String>, Vec<String>) {
    let parsed = parse(source);
    let names = parsed
        .file
        .items
        .iter()
        .map(|item| match &item.decl {
            Decl::Structure(s) => s.name.name.clone(),
            Decl::Scalar(s) => s.name.name.clone(),
            Decl::Operation(o) => o.name.name.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    let messages = parsed
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    (names, messages)
}

#[test]
fn a_missing_closing_brace_does_not_swallow_the_next_declaration() {
    let (names, messages) =
        names("structure A {\n    a: string\n\nstructure B {\n    b: string\n}\n");
    assert_eq!(names, ["A", "B"]);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("`}`"), "{messages:?}");
}

#[test]
fn garbage_between_declarations_is_skipped_with_one_error() {
    let (names, messages) = names("string A\n= = =\nstring B\n");
    assert_eq!(names, ["A", "B"]);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(
        messages[0].starts_with("expected a declaration"),
        "{messages:?}"
    );
}

#[test]
fn every_bad_member_is_reported() {
    let (names, messages) = names("structure A {\n    a: \n    : 1\n    b: string\n}\n");
    assert_eq!(names, ["A"]);
    assert!(messages.len() >= 2, "{messages:?}");
}

#[test]
fn errors_name_what_was_expected_and_found() {
    let (_, messages) = names("operation Get {\n    http GET /cities\n}\n");
    assert!(
        messages
            .iter()
            .any(|m| m == "expected a URI pattern in quotes, found an unexpected character"),
        "{messages:?}"
    );
}

#[test]
fn a_no_clause_rejects_must() {
    let (_, messages) = names("rule R {\n    no document must documented\n}\n");
    assert!(
        messages.iter().any(|m| m.contains("`no` clause")),
        "{messages:?}"
    );
}
