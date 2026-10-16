//! Policy rules and exceptions (decisions 45-47): rules over the compiled
//! model, the vocabulary they are checked against, the levels, and the
//! exceptions recorded in the compiled artifact.

use crucible_lang::{Compiled, Package, Severity, SourceFile, compile};

const MODEL: &str = r#"
service Shop {
    version "1"
    protocol json
    resources [Organization]
    operations [Ping]
}

/// How an operation is charged.
metadata class on operation: enum { read, write, job }
metadata exempt on operation
/// Why the operation is exempt.
metadata note on operation: string
/// Not for the runtime.
metadata audit on operation: enum { managed, open } retention source
metadata hidden on member

string Id { length 1..64 }
string Name { length 1..100 }
integer Count { range 0..1000 }
string Secret { length 1..64 }
string Loose
timestamp Stamp
list Names: Name { length 0..10 }
enum Color { red, green }

resource Organization {
    id organizationId: Id
    property name: Name
}

resource Order in Organization {
    id orderId: Id
    readonly property count: Count
}

/// An order, as reported.
structure OrderData for Order {
    $orderId
    $count
}

@class(read)
@audit(managed)
operation GetOrder reads Order {
    http GET "/v1/organizations/{organizationId}/orders/{orderId}"
    input { $organizationId $orderId }
    output { order: OrderData }
}

@class(write)
idempotent operation PutOrderNote on Order {
    http PUT "/v1/organizations/{organizationId}/orders/{orderId}/note"
    input { $organizationId $orderId note: Name }
}

@class(read)
readonly operation Ping {
    http GET "/ping"
    output { color: Color }
}
"#;

fn compiled(policy: &str) -> Compiled {
    compiled_with(MODEL, policy)
}

fn compiled_with(model: &str, policy: &str) -> Compiled {
    compile(
        &Package::new(
            "example.shop",
            vec![
                SourceFile {
                    path: "shop.cru".to_owned(),
                    text: model.to_owned(),
                },
                SourceFile {
                    path: "policy.cru".to_owned(),
                    text: policy.to_owned(),
                },
            ],
        ),
        None,
    )
}

fn errors(policy: &str) -> Vec<String> {
    messages(&compiled(policy), true)
}

fn messages(compiled: &Compiled, errors: bool) -> Vec<String> {
    compiled
        .diagnostics
        .iter()
        .filter(|item| item.diagnostic.is_error() == errors)
        .map(|item| item.diagnostic.message.clone())
        .collect()
}

fn mentions(found: &[String], needle: &str) {
    assert!(
        found.iter().any(|m| m.contains(needle)),
        "nothing mentions {needle:?}: {found:#?}"
    );
}

fn clean(policy: &str) {
    let found = errors(policy);
    assert!(found.is_empty(), "{found:#?}");
}

// ---- the rules fire -------------------------------------------------------

#[test]
fn a_rule_that_holds_is_silent() {
    clean("rule Classified strict { every operation must has @class }");
}

#[test]
fn a_violation_names_the_element_the_condition_and_the_rules_doc() {
    let found = errors(
        r#"
/// Every write says why it is exempt.
rule WritesExplain strict {
    every operation where @class == write
    must has @note
}
"#,
    );
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0]
            .starts_with("rule WritesExplain: `PutOrderNote` does not satisfy `must has @note`"),
        "{}",
        found[0]
    );
    assert!(found[0].contains("Every write says why it is exempt."));
}

#[test]
fn a_violation_is_reported_where_the_element_is_declared() {
    let compiled = compiled("rule Classified strict { every operation must has @exempt }");
    let first = compiled
        .diagnostics
        .iter()
        .find(|d| d.diagnostic.message.starts_with("rule Classified"))
        .expect("a violation");
    assert_eq!(first.file, Some(0), "in shop.cru, not policy.cru");
}

#[test]
fn has_one_of_and_not_and_or() {
    clean("rule A { every operation must has one of @class, @exempt }");
    clean(
        "rule B { every operation where @class == read or @class == write must not has @exempt }",
    );
    let found = errors("rule C { every operation must has one of @exempt, @note }");
    assert_eq!(found.len(), 3, "{found:#?}");
}

#[test]
fn flags_and_http_paths() {
    // GetOrder and Ping are readonly; the PUT is idempotent.
    clean(
        r#"
rule Methods {
    every operation where readonly must http.method == GET
    every operation where http.method == PUT must idempotent
    every operation where http.uri startsWith "/v1/organizations/" must http.status == 200
}
"#,
    );
    let found = errors("rule M { every operation where readonly must http.method == POST }");
    assert_eq!(found.len(), 2, "{found:#?}");
}

#[test]
fn role_and_addresses_follow_the_resources() {
    // Orders are below Organization, so both attached operations address it;
    // Ping, bound to no resource, does not.
    clean(
        r#"
rule Tenant strict {
    every operation where addresses Organization
    must http.uri startsWith "/v1/organizations/{organizationId}"
    every operation where role == reads
    must name startsWith "Get"
}
"#,
    );
    let found = errors(
        r#"rule Tenant { every operation where addresses Order must http.uri startsWith "/ping" }"#,
    );
    assert_eq!(found.len(), 2, "{found:#?}");
}

#[test]
fn member_conditions() {
    // `note` targets Name, a named shape; every member has one.
    clean("rule Named { every member must not type is inline primitive }");
    clean("rule Lengths { every member where type == Name must type.length.max >= 100 }");
    clean("rule Optional { every input member must required }");
    clean("rule Kinds { every member where name == color must type is enum }");
    let found =
        errors("rule Timed { every member where type is string must type.length.max < 50 }");
    mentions(&found, "does not satisfy");
}

#[test]
fn inline_primitives_are_found() {
    let model = MODEL.replace("note: Name", "note: string");
    let compiled = compiled_with(
        &model,
        "rule Named strict { every member must not type is inline primitive }",
    );
    let found = messages(&compiled, true);
    assert_eq!(found.len(), 1, "{found:#?}");
    mentions(&found, "`PutOrderNoteInput.note`");
}

#[test]
fn constraints_are_read_from_shapes() {
    clean("rule A { every string where not enum must has length }");
    clean("rule B { every number must has range }");
    clean("rule C { every list must has length }");
    clean("rule D { every string where name == Id must length.max == 64 }");
    let found = errors("rule Bounded strict { every string where not enum must has pattern }");
    // Id and Name have no pattern; the string enum is out by `not enum`.
    mentions(&found, "`Id`");
    mentions(&found, "`Name`");
    assert!(!found.iter().any(|m| m.contains("`Color`")), "{found:#?}");
    mentions(
        &errors("rule Bounded strict { every enum must has pattern }"),
        "does not apply to an enum",
    );
}

/// A bound compares exactly with a rule's literal: in `f64`,
/// `0.29999999999999999999` and `0.3` are the same number.
#[test]
fn range_bounds_compare_exactly_with_a_literal() {
    let model = format!(
        "{}\nbigDecimal Rate {{ range ..0.29999999999999999999 }}\n",
        MODEL.replace(
            "output { color: Color }",
            "output { color: Color rate: Rate }"
        )
    );
    let errors = |policy: &str| messages(&compiled_with(&model, policy), true);
    let holds = |policy: &str| {
        let found = errors(policy);
        assert!(found.is_empty(), "{policy}: {found:#?}");
    };
    holds("rule Below { every number where name == Rate must range.max < 0.3 }");
    holds(
        "rule Exact { every number where name == Rate must range.max == 0.29999999999999999999 }",
    );
    holds("rule Above { every number where name == Rate must range.max > 0.29999999999999999998 }");
    mentions(
        &errors("rule Equal { every number where name == Rate must range.max == 0.3 }"),
        "does not satisfy",
    );
    mentions(
        &errors("rule Huge { every number must range.max < 1e99999999999999999999 }"),
        "`1e99999999999999999999` is out of range",
    );
}

#[test]
fn a_loose_string_is_caught_by_has_length() {
    let model = format!("{MODEL}\nstructure WideData {{ loose: Loose }}\nreadonly operation Wide {{ http GET \"/wide\"\n output {{ wide: WideData }} }}\n").replace("operations [Ping]", "operations [Ping, Wide]");
    let compiled = compiled_with(
        &model,
        "rule Strings strict { every string where not enum must has length }",
    );
    let found = messages(&compiled, true);
    assert_eq!(found.len(), 1, "{found:#?}");
    mentions(&found, "`Loose`");
}

#[test]
fn documents_can_be_forbidden() {
    let model = format!("{MODEL}\ndocument Open\nstructure Holder {{ open: Open }}\nreadonly operation Hold {{ http GET \"/hold\"\n output {{ holder: Holder }} }}\n").replace("operations [Ping]", "operations [Ping, Hold]");
    let found = messages(
        &compiled_with(&model, "rule NoDocument { no document }"),
        true,
    );
    assert_eq!(found.len(), 1, "{found:#?}");
    mentions(&found, "`Open` is a document, which this rule forbids");
}

#[test]
fn an_unreferenced_shape_is_found() {
    let model =
        format!("{MODEL}\nstructure Orphan {{ name: Name }}\nstring Lonely {{ length 1..2 }}\n");
    let found = messages(
        &compiled_with(
            &model,
            "rule Referenced strict { no shape where not referenced }",
        ),
        true,
    );
    mentions(&found, "`Orphan`");
    mentions(&found, "`Lonely`");
}

#[test]
fn shapes_reached_only_through_an_include_or_a_resource_are_referenced() {
    let model = format!(
        "{MODEL}\nstructure Bundle {{ extra?: Name }}\nstring Tag {{ length 1..4 }}\nresource Thing {{ id thingId: Id\n property tag: Tag }}\n"
    );
    // Bundle is not included anywhere, Tag is a resource entry's type.
    let found = messages(
        &compiled_with(
            &model,
            "rule Referenced strict { no shape where not referenced }",
        ),
        true,
    );
    mentions(&found, "`Bundle`");
    assert!(!found.iter().any(|m| m.contains("`Tag`")), "{found:#?}");
}

#[test]
fn documentation_is_a_flag() {
    let found = errors("rule Docs { every operation must documented }");
    assert_eq!(found.len(), 3, "{found:#?}");
    clean("rule Docs { every structure where name == OrderData must documented }");
}

#[test]
fn a_body_that_contains_a_sensitive_member_is_found() {
    let model = MODEL.replace("note: Name", "note: Secret").replace(
        "string Secret { length 1..64 }",
        "sensitive string Secret { length 1..64 }",
    );
    let found = messages(
        &compiled_with(
            &model,
            "rule Quiet { every operation where role == lists must not output contains sensitive\n every operation must not input contains sensitive }",
        ),
        true,
    );
    assert_eq!(found.len(), 1, "{found:#?}");
    mentions(&found, "`PutOrderNote`");
}

#[test]
fn source_retained_metadata_is_read_and_never_reaches_the_ir() {
    // `audit` is retention source: GetOrder carries it.
    clean("rule Audited { every operation where name == GetOrder must @audit == managed }");
    let found =
        errors("rule Audited { every operation where name == GetOrder must @audit == open }");
    assert_eq!(found.len(), 1, "{found:#?}");
    let compiled = compiled("");
    let service = compiled.service.expect("compiles");
    let get = service
        .operations
        .iter()
        .find(|o| o.name() == "GetOrder")
        .unwrap();
    assert!(get.traits.keys().any(|k| k.name() == "class"));
    assert!(!get.traits.keys().any(|k| k.name() == "audit"));
}

// ---- the vocabulary is checked --------------------------------------------

#[test]
fn a_rule_that_could_never_match_is_an_error_in_the_rule() {
    mentions(
        &errors("rule R { every operation must has @nosuch }"),
        "`@nosuch` is not declared with `metadata`",
    );
    mentions(
        &errors("rule R { every operation must @class == sideways }"),
        "`@class` is one of read, write, job",
    );
    mentions(
        &errors("rule R { every member must has @class }"),
        "`@class` is declared on operation, which a member is not",
    );
    mentions(
        &errors("rule R { every operation must readonly or reallyreadonly }"),
        "unknown condition `reallyreadonly`",
    );
    mentions(
        &errors("rule R { every member must readonly }"),
        "`readonly` does not apply to a member",
    );
    mentions(
        &errors("rule R { every widget must documented }"),
        "`widget` is not something a rule ranges over",
    );
    mentions(
        &errors("rule R { every operation where addresses Nowhere must readonly }"),
        "`Nowhere` is not a resource",
    );
    mentions(
        &errors("rule R { every string must has @class }"),
        "declared on operation",
    );
}

#[test]
fn deprecated_on_a_resource_is_not_called_a_member_annotation() {
    let found = errors("rule R { every resource must has @deprecated }");
    mentions(&found, "`@deprecated` is not on a resource");
    assert!(
        !found.iter().any(|m| m.contains("is on a member")),
        "{found:#?}"
    );
    mentions(
        &errors("rule R { every operation must has @header }"),
        "`@header` is on a member, not an operation",
    );
}

#[test]
fn operators_and_operands_are_checked() {
    mentions(
        &errors("rule R { every operation must http.status startsWith \"2\" }"),
        "cannot be compared with `startsWith`",
    );
    mentions(
        &errors("rule R { every operation must http.status == ok }"),
        "is a number",
    );
    mentions(
        &errors("rule R { every operation must name matches \"(\" }"),
        "the pattern does not run",
    );
    mentions(
        &errors("rule R { every operation where role == sometimes must readonly }"),
        "is not a role",
    );
    mentions(
        &errors("rule R { every operation must name }"),
        "has a value, not a yes or no",
    );
    mentions(
        &errors("rule R { every operation must @exempt == true }"),
        "has no value to compare",
    );
    mentions(
        &errors("rule R { every member must type is gadget }"),
        "names no kind of type",
    );
    mentions(
        &errors("rule R { every operation must input contains nothing }"),
        "a body can only be tested with",
    );
}

#[test]
fn a_clause_needs_what_it_says() {
    mentions(&errors("rule R { every operation }"), "add `must ..`");
    mentions(&errors("rule R { }"), "has no clause");
    mentions(&errors("rule R strict strict { no document }"), "one level");
    mentions(
        &errors("rule R sometimes { no document }"),
        "is not a rule level",
    );
    mentions(
        &errors("rule R { no document }\nrule R { no document }"),
        "declared twice",
    );
}

#[test]
fn rules_wait_for_a_model_that_compiles() {
    let model = MODEL.replace("note: Name", "note: Missing");
    let found = messages(
        &compiled_with(&model, "rule R strict { every operation must has @exempt }"),
        true,
    );
    mentions(&found, "unknown type `Missing`");
    assert!(!found.iter().any(|m| m.starts_with("rule R")), "{found:#?}");
}

// ---- levels and exceptions ------------------------------------------------

#[test]
fn advice_never_fails_the_compile() {
    let compiled = compiled("rule Nudge advice { every operation must has @exempt }");
    assert!(!compiled.has_errors());
    assert!(compiled.service.is_some());
    let advice: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.severity == Severity::Advice)
        .collect();
    assert_eq!(advice.len(), 3);
}

#[test]
fn an_exception_excuses_one_violation_and_is_recorded() {
    let policy = r#"
rule Classified { every operation must has @exempt }
except Classified for GetOrder because "Reads of an order are exempt by definition."
except Classified for PutOrderNote because "Notes are exempt."
except Classified for Ping because "A health check."
"#;
    let compiled = compiled(policy);
    assert!(!compiled.has_errors(), "{}", compiled.render());
    let service = compiled.service.expect("compiles");
    let recorded: Vec<_> = service
        .exceptions
        .iter()
        .map(|e| (e.rule.as_str(), e.target.as_str(), e.package.as_str()))
        .collect();
    assert_eq!(
        recorded,
        [
            ("Classified", "GetOrder", "example.shop"),
            ("Classified", "Ping", "example.shop"),
            ("Classified", "PutOrderNote", "example.shop"),
        ]
    );
    assert_eq!(service.exceptions[1].reason, "A health check.");
}

#[test]
fn an_exception_for_an_operation_covers_its_input_and_output_members() {
    let model = MODEL.replace("note: Name", "note: string");
    let policy = r#"
rule Named { every input member must not type is inline primitive }
except Named for PutOrderNote because "A free-form note."
"#;
    let compiled = compiled_with(&model, policy);
    assert!(!compiled.has_errors(), "{}", compiled.render());
}

#[test]
fn a_strict_rule_has_no_exceptions() {
    let found = errors(
        r#"
rule Classified strict { every operation must has @exempt }
except Classified for Ping because "No."
"#,
    );
    mentions(
        &found,
        "rule `Classified` is strict: no exception can excuse it",
    );
    // The violation still stands.
    mentions(&found, "rule Classified: `Ping`");
}

#[test]
fn advice_has_nothing_to_except() {
    mentions(
        &errors(
            "rule Nudge advice { every operation must has @exempt }\nexcept Nudge for Ping because \"Fine.\"",
        ),
        "is advice and never fails the compile",
    );
}

#[test]
fn a_stale_exception_is_an_error() {
    let found = errors(
        r#"
rule Classified { every operation must has @class }
except Classified for Ping because "It is classified, so this excuses nothing."
except Classified for NoSuchOperation because "Nor does this."
"#,
    );
    assert_eq!(found.len(), 2, "{found:#?}");
    mentions(&found, "`except Classified for Ping` excuses nothing");
    mentions(
        &found,
        "`except Classified for NoSuchOperation` excuses nothing",
    );
}

#[test]
fn an_exception_needs_a_known_rule_a_reason_and_no_repeat() {
    mentions(
        &errors("rule R { no document }\nexcept Q for Ping because \"x\""),
        "unknown rule `Q`",
    );
    mentions(
        &errors("rule R { every operation must has @exempt }\nexcept R for Ping because \"  \""),
        "needs a reason",
    );
    mentions(
        &errors(
            "rule R { every operation must has @exempt }\nexcept R for Ping because \"a\"\nexcept R for Ping because \"b\"",
        ),
        "twice",
    );
}

#[test]
fn rules_and_exceptions_survive_formatting() {
    let source = "rule R strict {\n    every operation where readonly\n    must has @class\n}\n\nexcept R for Ping because \"x\"\n";
    let formatted = crucible_lang::format(source, &crucible_lang::FormatOptions::default());
    assert_eq!(formatted, source);
}

#[test]
fn an_inline_built_in_type_is_a_kind_of_type() {
    let model = MODEL.replace("note: Name", "note: string");
    let found = messages(
        &compiled_with(
            &model,
            "rule Plain { every member where type is string must not name == note }",
        ),
        true,
    );
    assert_eq!(found.len(), 1, "{found:#?}");
    mentions(&found, "`PutOrderNoteInput.note`");
}

#[test]
fn a_blank_doc_does_not_document_a_resource() {
    let model = MODEL.replace("resource Organization {", "///\nresource Organization {");
    let found = messages(
        &compiled_with(&model, "rule Docs { every resource must documented }"),
        true,
    );
    mentions(&found, "`Organization`");
    mentions(&found, "`Order`");
}

#[test]
fn a_members_own_name_does_not_excuse_it() {
    let model = MODEL.replace("note: Name", "note: string");
    let policy = r#"
rule Named { every member must not type is inline primitive }
except Named for note because "Only a shape or operation can be named."
"#;
    let found = messages(&compiled_with(&model, policy), true);
    mentions(&found, "`except Named for note` excuses nothing");
    mentions(&found, "rule Named: `PutOrderNoteInput.note`");
}

#[test]
fn an_unknown_side_does_not_hide_what_the_other_decides() {
    // `Orphan` is never lowered, so `has length` is unknown for it; `not
    // referenced` decides the `or`.
    let model = format!("{MODEL}\nstring Orphan {{ length 1..2 }}\n");
    let found = messages(
        &compiled_with(
            &model,
            "rule R { every string where not enum and (has length or not referenced) must documented }",
        ),
        true,
    );
    mentions(&found, "`Orphan`");
}

#[test]
fn an_error_members_header_and_payload_bindings_are_visible_to_rules() {
    let model = format!(
        "{}
error Busy {{
    status 429
    @header(\"Retry-After\") retryAfter: Count
    reason: Name
}}
error Raw {{
    status 400
    @payload data: blob
}}
@class(read)
readonly operation Probe {{
    http GET \"/probe\"
    errors [Busy, Raw]
}}
",
        MODEL.replace("operations [Ping]", "operations [Ping, Probe]")
    );
    let found = |policy: &str| messages(&compiled_with(&model, policy), true);
    let held = found("rule A { every member where name == retryAfter must has @header }");
    assert!(held.is_empty(), "{held:#?}");
    let held = found("rule B { every member where name == data must has @payload }");
    assert!(held.is_empty(), "{held:#?}");
    // The unbound member still fails: the binding is read, not assumed.
    let loose = found("rule C { every member where name == reason must has @header }");
    mentions(&loose, "`Busy.reason` does not satisfy");
}
