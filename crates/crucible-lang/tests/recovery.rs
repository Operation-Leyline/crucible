//! Parsing reports every problem and keeps going.

use crucible_lang::ast::{Decl, MemberKind, OpEntry};
use crucible_lang::diagnostic::Severity;
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
            .any(|m| m == "expected a URI pattern in quotes, found `cities`"),
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

#[test]
fn deep_nesting_is_an_error_not_a_crash() {
    let nots = format!(
        "rule R {{\n    every operation\n    must {}readonly\n}}\n",
        "not ".repeat(5000)
    );
    let parens = format!(
        "rule R {{\n    every operation\n    must {}readonly{}\n}}\n",
        "(".repeat(300),
        ")".repeat(300)
    );
    let lists = format!(
        "operation Op {{\n    example \"x\" {{ input {{ a: {} }} }}\n}}\n",
        "[".repeat(3000)
    );
    let objects = format!(
        "operation Op {{\n    example \"x\" {{ input {} }}\n}}\n",
        "{ a: ".repeat(1000)
    );
    for source in [nots, parens, lists, objects] {
        let parsed = parse(&source);
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|d| d.message.contains("more than 64")),
            "{:?}",
            parsed
                .diagnostics
                .iter()
                .map(|d| &d.message)
                .take(3)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn an_unclosed_parenthesis_or_bracket_stops_at_the_next_declaration() {
    let (items, messages) = names(
        "operation Op {\n    paginated(items: x\n}\n\nstructure A {\n    a: string\n}\n\nstructure B {\n    b: string\n}\n",
    );
    assert_eq!(items, ["Op", "A", "B"], "{messages:?}");
    let (items, _) =
        names("service S {\n    operations [Ping\n}\n\nstructure A {\n    a: string\n}\n");
    assert_eq!(items.last().map(String::as_str), Some("A"));
}

#[test]
fn layout_carries_no_meaning() {
    for source in [
        "structure Col {\n@deprecated\nname: string\n}\n",
        "structure Col {\nlist: string\nerror: string\n}\n",
        "operation Op {\n    http GET \"/\"\n    errors [Oops]\n    example \"x\" {\nerror Oops { message: \"m\" }\n    }\n}\n",
        "service S {\nprotocol json\nversion \"1\"\n}\n",
    ] {
        let parsed = parse(source);
        assert!(
            parsed.diagnostics.is_empty(),
            "{source}\n{:?}",
            parsed.diagnostics
        );
    }
}

#[test]
fn a_failed_declaration_is_reported_once() {
    let (_, messages) = names("@foo bar\n");
    assert_eq!(messages.len(), 1, "{messages:?}");
    let (_, messages) = names("error: string\n");
    assert_eq!(messages.len(), 1, "{messages:?}");
}

#[test]
fn members_and_enum_values_take_modifiers() {
    let parsed = parse(
        "structure S {\n    sensitive pw: string\n}\n\nenum E {\n    internal FOO\n    BAR\n}\n",
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let Decl::Structure(s) = &parsed.file.items[0].decl else {
        panic!()
    };
    assert_eq!(s.members[0].modifiers[0].name, "sensitive");
    let Decl::Enum(e) = &parsed.file.items[1].decl else {
        panic!()
    };
    assert_eq!(e.values.len(), 2);
    assert_eq!(e.values[0].modifiers[0].name, "internal");
}

#[test]
fn a_missing_colon_is_reported_and_the_type_kept() {
    let parsed = parse("structure S {\n    a string\n}\n");
    assert_eq!(parsed.diagnostics.len(), 1, "{:?}", parsed.diagnostics);
    assert!(parsed.diagnostics[0].message.contains("`:`"));
    let Decl::Structure(s) = &parsed.file.items[0].decl else {
        panic!()
    };
    let MemberKind::Field { ty, .. } = &s.members[0].kind else {
        panic!()
    };
    assert_eq!(ty.as_ref().unwrap().name.name, "string");
}

#[test]
fn any_word_in_the_role_position_is_kept_for_lowering_to_check() {
    let parsed = parse("operation Op reeds Graph {\n    http GET \"/\"\n}\n");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let Decl::Operation(op) = &parsed.file.items[0].decl else {
        panic!()
    };
    assert_eq!(op.attachment.as_ref().unwrap().role.name, "reeds");
}

#[test]
fn doc_comments_attach_or_are_reported() {
    let parsed = parse("/// Kept.\n// a note\nstructure X {}\n");
    assert_eq!(parsed.file.items[0].doc.as_ref().unwrap().text, "Kept.");
    assert!(parsed.diagnostics.is_empty());

    let parsed = parse(
        "operation Op {\n    /// The request.\n    input {}\n    /// Lost.\n    http GET \"/\"\n}\n/// Orphan.\n",
    );
    let Decl::Operation(op) = &parsed.file.items[0].decl else {
        panic!()
    };
    let OpEntry::Body(body) = &op.entries[0] else {
        panic!()
    };
    assert_eq!(body.doc.as_ref().unwrap().text, "The request.");
    let warnings: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert_eq!(warnings.len(), 2, "{:?}", parsed.diagnostics);
}

#[test]
fn lexer_edge_cases() {
    assert!(parse("\u{feff}structure A {}\n").diagnostics.is_empty());
    let parsed = parse("§§§ structure A {}\n");
    assert_eq!(
        parsed
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("unexpected"))
            .count(),
        1
    );
    let parsed = parse("string S { pattern \"\\u{+41}\" }\n");
    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|d| d.message.contains("not a character")),
        "{:?}",
        parsed.diagnostics
    );
}

#[test]
fn rule_flags_and_metadata_targets_do_not_take_the_next_declaration() {
    let (items, _) = names("rule R strict\nstructure X {\n    a: string\n}\n");
    assert_eq!(items.last().map(String::as_str), Some("X"));
    let (items, _) = names("metadata k on\nstructure X {\n    a: string\n}\n");
    assert_eq!(items.last().map(String::as_str), Some("X"));
}
