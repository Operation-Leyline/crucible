//! The compatibility guard with declared migrations: a breaking change
//! compiles only when the migration from the baseline declares it, and a
//! declared step must match what changed.

use std::path::{Path, PathBuf};

use crucible_lang::{Guard, compile, guard, load_package};
use crucible_policy::Config;

const V1: &str = r#"service Shop {
    version "v1"
    protocol json
    operations [GetItem, ListItems, DeleteItem]
    errors [ValidationException, InternalServerException]
}

readonly operation GetItem {
    http GET "/items/{id}"
    input { id: string }
    output {
        name: string
        size?: integer
        kind: Kind
    }
}

enum Kind { SMALL, LARGE }

readonly operation ListItems {
    http GET "/items"
    input {
        @query
        limit?: integer
    }
    output { names: Names }
}

list Names: string

idempotent operation DeleteItem {
    http DELETE "/items/{id}"
    input { id: string }
    output { deleted: boolean }
}
"#;

const MANIFEST: &str = "[model]\nnamespace = \"example.shop\"\nimports = [\"crucible.errors\"]\n";

/// Freezes `source`, a release of the shop, into `dir/releases`, as
/// `crucible freeze` does: compiled once, then a snapshot.
fn freeze(dir: &Path, source: &str) {
    let staging = dir.with_extension("staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("crucible.toml"), MANIFEST).unwrap();
    std::fs::write(staging.join("shop.cru"), source).unwrap();
    let compiled = compile(&load_package(&staging).unwrap(), None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let service = compiled.service.unwrap();
    crucible_lang::release::freeze(&dir.join("releases"), &service, "test").unwrap();
}

/// A package whose baseline, `releases/v1.json`, is `V1`, and whose model
/// is `current` plus `migration`.
fn package(name: &str, current: &str, migration: &str) -> PathBuf {
    package_from(name, V1, current, migration)
}

fn package_from(name: &str, baseline: &str, current: &str, migration: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("migrations-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    freeze(&dir, baseline);
    std::fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v1.json\"\n"),
    )
    .unwrap();
    // A release with a migration is the next version.
    let current = if migration.is_empty() {
        current.to_owned()
    } else {
        current.replace("version \"v1\"", "version \"v2\"")
    };
    std::fs::write(dir.join("shop.cru"), format!("{current}\n{migration}")).unwrap();
    dir
}

/// An enum whose second value's wire value spells the first's symbol.
const COLOR: &str = "\nenum Color { RED = \"r\", SPECIAL = \"RED\" }\n";

/// COLOR reached directly, through a list and through a map.
const PAINT: &str = r#"
enum Color { RED = "r", SPECIAL = "RED" }
list Colors: Color
map Palette: string -> Color
structure Paint {
    color: Color
    colors?: Colors
    palette?: Palette
}
"#;

fn requirement_package(name: &str, ty: &str, declarations: &str, default: &str) -> PathBuf {
    let baseline = V1
        .replace("http GET \"/items\"", "http POST \"/items\"")
        .replace("@query\n        limit?: integer", &format!("limit?: {ty}"))
        + declarations;
    let current = baseline.replace("        limit?:", "        limit:");
    package_from(
        name,
        &baseline,
        &current,
        &format!(
            "migration \"v2\" from \"v1\" {{\n    require ListItems.input.limit default {default}\n}}\n"
        ),
    )
}

#[test]
fn requirement_defaults_must_fit_the_destination_and_effective_constraints() {
    for (name, ty, declarations, default, expected) in [
        (
            "wrong-type",
            "integer",
            "",
            "\"not an integer\"",
            "must be a integer",
        ),
        ("null", "integer", "", "null", "must be a integer"),
        (
            "range",
            "integer { range 1..100 }",
            "",
            "0",
            "outside its range",
        ),
        (
            "type-range",
            "integer",
            "",
            "2147483648",
            "outside the range of the type",
        ),
        (
            "length",
            "string { length 2..4 }",
            "",
            "\"x\"",
            "length bounds",
        ),
        ("enum", "Kind", "", "UNKNOWN", "not a value"),
        // A quoted string is a wire value, never a symbol: `SPECIAL` is a
        // symbol of Color, but no value of it is "SPECIAL".
        (
            "enum-quoted-symbol",
            "Color",
            COLOR,
            "\"SPECIAL\"",
            "not a value",
        ),
        // An optional document may be null; a required one may not, as in
        // Smithy.
        (
            "null-required-document",
            "Nested",
            "\nstructure Nested { data: document }\n",
            "{ data: null }",
            "must not be null",
        ),
        // A symbol is resolved wherever an enum is expected, so a quoted
        // symbol name is refused at depth too.
        (
            "nested-enum-quoted-symbol",
            "Paint",
            PAINT,
            "{ color: \"SPECIAL\" }",
            "not a value",
        ),
        (
            "list-item",
            "Numbers",
            "\ninteger Count { range 1..10 }\nlist Numbers: Count\n",
            "[0]",
            "outside its range",
        ),
        (
            "map-value",
            "Counts",
            "\ninteger Count { range 1..10 }\nmap Counts: string -> Count\n",
            "{ small: 0 }",
            "outside its range",
        ),
        (
            "nested",
            "Nested",
            "\nstructure Nested { count: integer { range 1..10 } }\n",
            "{ count: 0 }",
            "outside its range",
        ),
    ] {
        let dir = requirement_package(
            &format!("require-default-{name}"),
            ty,
            declarations,
            default,
        );
        let loaded = load_package(&dir).unwrap();
        let compiled = compile(&loaded, None);
        assert!(compiled.service.is_some(), "{name}: {}", compiled.render());
        let checked = guard(&loaded, &compiled, loaded.files.len(), &Config::default())
            .unwrap()
            .unwrap();
        assert!(checked.has_errors(), "{name}: {:?}", problems(&checked));
        assert!(
            problems(&checked)
                .iter()
                .any(|problem| problem.contains(expected)),
            "{name}: {:?}",
            problems(&checked)
        );
        let error = crucible_lang::versions(&loaded, &compiled, loaded.files.len()).unwrap_err();
        assert!(error.contains(expected), "{name}: {error}");
    }
}

#[test]
fn legal_requirement_defaults_are_validated_and_lowered_to_wire_values() {
    for (name, ty, declarations, default, expected) in [
        (
            "integer",
            "integer { range 1..100 }",
            "",
            "20",
            serde_json::json!(20),
        ),
        (
            "enum-symbol",
            "Kind",
            "",
            "SMALL",
            serde_json::json!("SMALL"),
        ),
        (
            "enum-wire",
            "Size",
            "\nenum Size { SMALL = \"s\", LARGE = \"l\" }\n",
            "SMALL",
            serde_json::json!("s"),
        ),
        // A symbol names its wire value; a quoted string is the wire value
        // itself, even when it spells another symbol's name.
        (
            "enum-symbol-shadowed",
            "Color",
            COLOR,
            "RED",
            serde_json::json!("r"),
        ),
        (
            "enum-quoted-wire",
            "Color",
            COLOR,
            "\"RED\"",
            serde_json::json!("RED"),
        ),
        (
            "document-null",
            "document",
            "",
            "{ data: null, items: [null, { x: null }] }",
            serde_json::json!({"data": null, "items": [null, {"x": null}]}),
        ),
        (
            "nested-document-null",
            "Nested",
            "\nstructure Nested { data: document }\n",
            "{ data: { inner: null } }",
            serde_json::json!({"data": {"inner": null}}),
        ),
        // An explicit null is a document value, kept on the wire.
        (
            "null-optional-document",
            "Nested",
            "\nstructure Nested { data?: document }\n",
            "{ data: null }",
            serde_json::json!({"data": null}),
        ),
        // Symbols nested in structures, lists and maps are resolved against
        // the shapes they sit in; quoted strings stay wire values.
        (
            "nested-enum",
            "Paint",
            PAINT,
            r#"{ color: RED, colors: [RED, "RED"], palette: { a: RED, b: "RED" } }"#,
            serde_json::json!({
                "color": "r",
                "colors": ["r", "RED"],
                "palette": {"a": "r", "b": "RED"}
            }),
        ),
        (
            "nested-enum-quoted-wire",
            "Paint",
            PAINT,
            r#"{ color: "RED" }"#,
            serde_json::json!({"color": "RED"}),
        ),
        (
            "list",
            "Numbers",
            "\ninteger Count { range 1..10 }\nlist Numbers: Count\n",
            "[1, 10]",
            serde_json::json!([1, 10]),
        ),
        (
            "map",
            "Counts",
            "\ninteger Count { range 1..10 }\nmap Counts: string -> Count\n",
            "{ small: 1 }",
            serde_json::json!({"small": 1}),
        ),
        (
            "nested",
            "Nested",
            "\nstructure Nested { count: integer { range 1..10 } }\n",
            "{ count: 1 }",
            serde_json::json!({"count": 1}),
        ),
        (
            "nested-timestamp",
            "Nested",
            "\nstructure Nested { created: timestamp }\n",
            "{ created: 0 }",
            serde_json::json!({"created": "1970-01-01T00:00:00Z"}),
        ),
        (
            "timestamp-list",
            "Times",
            "\nlist Times: timestamp\n",
            "[0]",
            serde_json::json!(["1970-01-01T00:00:00Z"]),
        ),
    ] {
        let dir = requirement_package(&format!("require-valid-{name}"), ty, declarations, default);
        let loaded = load_package(&dir).unwrap();
        let compiled = compile(&loaded, None);
        assert!(compiled.service.is_some(), "{name}: {}", compiled.render());
        let checked = guard(&loaded, &compiled, loaded.files.len(), &Config::default())
            .unwrap()
            .unwrap();
        assert!(
            !checked.has_errors(),
            "{name}: {:?} {:?}",
            rules(&checked),
            problems(&checked)
        );
        let versions = crucible_lang::versions(&loaded, &compiled, loaded.files.len()).unwrap();
        assert!(
            matches!(
                &versions[0].steps[..],
                [crucible_smithy::ir::VersionStep::Require { default, .. }] if default == &expected
            ),
            "{name}: {:?}",
            versions[0].steps
        );
    }
}

#[test]
fn every_problem_in_a_requirement_default_is_collected() {
    let dir = requirement_package(
        "require-collected",
        "Nested",
        "\nstructure Nested {\n    count: integer { range 1..10 }\n    label: string { length 1..10 }\n}\n",
        "{ count: 0, label: \"\" }",
    );
    let loaded = load_package(&dir).unwrap();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let checked = guard(&loaded, &compiled, loaded.files.len(), &Config::default())
        .unwrap()
        .unwrap();
    assert_eq!(checked.problems.len(), 2, "{:?}", problems(&checked));
    for member in ["count", "label"] {
        assert!(
            problems(&checked)
                .iter()
                .any(|problem| problem.contains(member))
        );
    }
    assert!(checked.problems.iter().all(|problem| {
        let source = &loaded.files[problem.file.unwrap()].text;
        let span = problem.diagnostic.span;
        &source[span.start as usize..span.end as usize] == "{ count: 0, label: \"\" }"
    }));
}

#[test]
fn timestamp_requirement_defaults_use_the_destination_wire_format() {
    for (name, ty, default, expected) in [
        (
            "date-time",
            "timestamp",
            "0",
            serde_json::json!("1970-01-01T00:00:00Z"),
        ),
        (
            "epoch",
            "timestamp { format epoch-seconds }",
            "\"1970-01-01T00:00:00Z\"",
            serde_json::json!(0),
        ),
    ] {
        let dir = requirement_package(&format!("require-timestamp-{name}"), ty, "", default);
        let loaded = load_package(&dir).unwrap();
        let compiled = compile(&loaded, None);
        assert!(compiled.service.is_some(), "{name}: {}", compiled.render());
        let checked = guard(&loaded, &compiled, loaded.files.len(), &Config::default())
            .unwrap()
            .unwrap();
        assert!(!checked.has_errors(), "{name}: {:?}", problems(&checked));
        let versions = crucible_lang::versions(&loaded, &compiled, loaded.files.len()).unwrap();
        assert!(
            matches!(
                &versions[0].steps[..],
                [crucible_smithy::ir::VersionStep::Require { default, .. }] if default == &expected
            ),
            "{name}: {:?}",
            versions[0].steps
        );
    }
}

#[test]
fn integer_enum_requirement_defaults_accept_symbols_and_emit_numbers() {
    let dir = requirement_package(
        "require-intenum",
        "Count",
        "\nenum Count: integer { SMALL = 1, LARGE = 2 }\n",
        "SMALL",
    );
    let loaded = load_package(&dir).unwrap();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let checked = guard(&loaded, &compiled, loaded.files.len(), &Config::default())
        .unwrap()
        .unwrap();
    assert!(!checked.has_errors(), "{:?}", problems(&checked));
    let versions = crucible_lang::versions(&loaded, &compiled, loaded.files.len()).unwrap();
    assert!(matches!(
        &versions[0].steps[..],
        [crucible_smithy::ir::VersionStep::Require { default, .. }] if default == &serde_json::json!(1)
    ));
}

#[test]
fn historical_requirement_defaults_cannot_bypass_validation() {
    let dir = package(
        "require-historical",
        &V1.replace("        limit?: integer", "        limit: integer"),
        "migration \"v2\" from \"v1\" {\n    require ListItems.input.limit default \"bad\"\n}\n",
    );
    let v2 = std::fs::read_to_string(dir.join("shop.cru")).unwrap();
    freeze(&dir, &v2);
    std::fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v2.json\"\n"),
    )
    .unwrap();
    let v3 = v2.replace("version \"v2\"", "version \"v3\"") + "\nmigration \"v3\" from \"v2\" {}\n";
    std::fs::write(dir.join("shop.cru"), v3).unwrap();
    let loaded = load_package(&dir).unwrap();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let latest = guard(&loaded, &compiled, loaded.files.len(), &Config::default())
        .unwrap()
        .unwrap();
    assert!(!latest.has_errors(), "{:?}", problems(&latest));
    let error = crucible_lang::versions(&loaded, &compiled, loaded.files.len()).unwrap_err();
    assert!(error.contains("the migration to \"v2\""), "{error}");
    assert!(error.contains("must be a integer"), "{error}");
}

fn run(name: &str, current: &str, migration: &str) -> Guard {
    let dir = package(name, current, migration);
    let loaded = load_package(&dir).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    guard(&loaded, &compiled, own, &Config::default())
        .expect("a baseline")
        .expect("the baseline compiles")
}

fn rules(guard: &Guard) -> Vec<String> {
    guard
        .undeclared
        .iter()
        .map(|finding| format!("{} {}", finding.rule, finding.shape))
        .collect()
}

fn problems(guard: &Guard) -> Vec<String> {
    guard
        .problems
        .iter()
        .map(|item| item.diagnostic.message.clone())
        .collect()
}

#[test]
fn an_unchanged_model_is_compatible() {
    let guard = run("unchanged", V1, "");
    assert_eq!(rules(&guard), Vec::<String>::new());
    assert!(!guard.has_errors());
    assert_eq!(guard.version, "v1");
}

#[test]
fn an_optional_member_after_the_others_needs_no_migration() {
    let added = V1.replace(
        "        kind: Kind\n    }",
        "        kind: Kind\n        color?: string\n    }",
    );
    assert_ne!(added, V1);
    let guard = run("optional-appended", &added, "");
    assert_eq!(rules(&guard), Vec::<String>::new());
    assert!(!guard.has_errors());

    // Before an existing member, it shifts positional constructors.
    let inserted = V1.replace(
        "        name: string\n",
        "        name: string\n        color?: string\n",
    );
    let guard = run("optional-inserted", &inserted, "");
    assert_eq!(
        rules(&guard),
        vec!["compatibility.sdk example.shop#GetItemOutput$color"]
    );
}

#[test]
fn an_undeclared_break_fails_and_a_declared_one_compiles() {
    let renamed = V1.replace("        name: string\n", "        title: string\n");
    let undeclared = run("rename-undeclared", &renamed, "");
    assert!(undeclared.has_errors(), "{:?}", rules(&undeclared));

    let declared = run(
        "rename-declared",
        &renamed,
        "migration \"v2\" from \"v1\" {\n    rename GetItem.output.name -> title\n}\n",
    );
    assert_eq!(rules(&declared), Vec::<String>::new());
    assert_eq!(problems(&declared), Vec::<String>::new());
    assert!(declared.declared > 0);
}

#[test]
fn routes_requirements_enum_values_and_removals_can_be_declared() {
    let current = V1
        .replace(
            "\"/items/{id}\"\n    input { id: string }\n    output {\n        name",
            "\"/things/{id}\"\n    input { id: string }\n    output {\n        name",
        )
        .replace("        limit?: integer", "        limit: integer")
        .replace(
            "enum Kind { SMALL, LARGE }",
            "enum Kind { SMALL, LARGE, HUGE }",
        )
        .replace(
            "operations [GetItem, ListItems, DeleteItem]",
            "operations [GetItem, ListItems]",
        );
    let current = current[..current.find("idempotent operation DeleteItem").unwrap()].to_owned();
    let undeclared = run("many-undeclared", &current, "");
    assert!(undeclared.has_errors());
    let guard = run(
        "many-declared",
        &current,
        r#"migration "v2" from "v1" {
    move GetItem from GET "/items/{id}"
    require ListItems.input.limit default 20
    add Kind.HUGE downgrade LARGE
    remove operation DeleteItem
    sunset "2028-01-01"
}
"#,
    );
    assert_eq!(
        rules(&guard),
        Vec::<String>::new(),
        "{:?}",
        problems(&guard)
    );
    assert_eq!(problems(&guard), Vec::<String>::new());
}

#[test]
fn a_declared_step_must_match_what_changed() {
    let guard = run(
        "phantom",
        V1,
        r#"migration "v2" from "v1" {
    rename GetItem.output.name -> title
    move ListItems from GET "/old"
    add Kind.LARGE downgrade SMALL
    remove operation GetItem
    sunset "soon"
}
"#,
    );
    let problems = problems(&guard);
    assert!(guard.has_errors());
    for expected in [
        "`name` is still a member",
        "ListItems was `GET \"/items\"` in the baseline",
        "Kind did not gain the value LARGE",
        "`GetItem` is still an operation",
        "YYYY-MM-DD",
    ] {
        assert!(
            problems.iter().any(|p| p.contains(expected)),
            "{expected}: {problems:#?}"
        );
    }
}

#[test]
fn one_release_has_one_migration() {
    let guard = run(
        "twice",
        V1,
        "migration \"v2\" from \"v1\" {}\nmigration \"v3\" from \"v1\" {}\n",
    );
    assert!(
        problems(&guard)
            .iter()
            .any(|p| p.contains("a second migration"))
    );
}

#[test]
fn reshapes_retypes_and_new_errors_can_be_declared() {
    let current = V1
        // name and size become one `label` structure.
        .replace(
            "        name: string\n        size?: integer\n        kind: Kind",
            "        label: Label\n        kind: Kind",
        )
        // A new error on GetItem.
        .replace(
            "    input { id: string }\n    output {\n        label",
            "    input { id: string }\n    errors [ItemGoneException]\n    output {\n        label",
        )
        // deleted becomes a status string.
        .replace("output { deleted: boolean }", "output { deleted: string }")
        + "\nstructure Label {\n    name: string\n    size?: integer\n}\n\nerror ItemGoneException {\n    status 410\n    message: string\n}\n";
    let undeclared = run("reshape-undeclared", &current, "");
    assert!(undeclared.has_errors());
    let guard = run(
        "reshape-declared",
        &current,
        r#"migration "v2" from "v1" {
    reshape GetItem.output { name, size } -> label by hook
    retype DeleteItem.output.deleted -> string by hook
    add GetItem.errors ItemGoneException downgrade InternalServerException {
        message <- message
    }
}
"#,
    );
    assert_eq!(
        rules(&guard),
        Vec::<String>::new(),
        "{:?}",
        problems(&guard)
    );
    assert_eq!(problems(&guard), Vec::<String>::new());

    // Older clients get the downgrade error built from the new one: the
    // step fills every member it requires, from members that exist.
    for (mapping, expected) in [
        (
            "message <- message",
            "ValidationException requires `reason`, which this step does not fill from ItemGoneException",
        ),
        (
            "message <- detail",
            "ItemGoneException has no member `detail`",
        ),
    ] {
        let guard = run(
            "unfilled-downgrade",
            &current,
            &format!(
                "migration \"v2\" from \"v1\" {{
    reshape GetItem.output {{ name, size }} -> label by hook
    retype DeleteItem.output.deleted -> string by hook
    add GetItem.errors ItemGoneException downgrade ValidationException {{
        {mapping}
    }}
}}
"
            ),
        );
        assert!(
            problems(&guard).iter().any(|p| p.contains(expected)),
            "{expected}: {:?}",
            problems(&guard)
        );
    }
}

#[test]
fn a_step_declares_only_its_own_kind_of_change() {
    // GetItem moves (declared) and gains an error (not declared): the move
    // must not account for the new error on the same operation.
    let current = V1
        .replace(
            "\"/items/{id}\"\n    input { id: string }\n    output {\n        name",
            "\"/things/{id}\"\n    input { id: string }\n    errors [ItemGoneException]\n    output {\n        name",
        )
        + "\nerror ItemGoneException {\n    status 410\n    message: string\n}\n";
    let guard = run(
        "kinds",
        &current,
        "migration \"v2\" from \"v1\" {\n    move GetItem from GET \"/items/{id}\"\n}\n",
    );
    let rules = rules(&guard);
    assert!(
        rules
            .iter()
            .any(|rule| rule.starts_with("compatibility.error")),
        "{rules:?}"
    );
    assert!(
        !rules
            .iter()
            .any(|rule| rule.starts_with("compatibility.http")),
        "{rules:?}"
    );
}

#[test]
fn the_versions_served_are_the_chain_of_frozen_releases() {
    use crucible_smithy::ir::VersionStep;

    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("migrations-chain");
    let _ = std::fs::remove_dir_all(&dir);
    // v1, then v2 renamed a member, then v3 (current) added an enum value.
    let to_v2 = "migration \"v2\" from \"v1\" {\n    rename GetItem.output.name -> title\n    sunset \"2027-01-01\"\n}\n";
    let v2 = V1
        .replace("version \"v1\"", "version \"v2\"")
        .replace("        name: string\n", "        title: string\n")
        + "\n"
        + to_v2;
    let to_v3 = "migration \"v3\" from \"v2\" {\n    add Kind.HUGE downgrade LARGE\n    require ListItems.input.limit default 20\n}\n";
    let v3 = v2
        .replace("version \"v2\"", "version \"v3\"")
        .replace(
            "enum Kind { SMALL, LARGE }",
            "enum Kind { SMALL, LARGE, HUGE }",
        )
        .replace("        limit?: integer", "        limit: integer")
        + "\n"
        + to_v3;
    std::fs::create_dir_all(&dir).unwrap();
    freeze(&dir, V1);
    freeze(&dir, &v2);
    std::fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v2.json\"\n"),
    )
    .unwrap();
    std::fs::write(dir.join("shop.cru"), v3).unwrap();

    let loaded = load_package(&dir).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let guard = guard(&loaded, &compiled, own, &Config::default())
        .unwrap()
        .unwrap();
    assert!(
        !guard.has_errors(),
        "{:?} {:?}",
        rules(&guard),
        problems(&guard)
    );

    let versions = crucible_lang::versions(&loaded, &compiled, own).unwrap();
    let names: Vec<&str> = versions.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["v1", "v2"]);
    assert_eq!(versions[0].sunset.as_deref(), Some("2027-01-01"));
    assert_eq!(versions[0].service.version, "v1");
    assert!(matches!(
        &versions[0].steps[..],
        [VersionStep::Rename { from, to, .. }] if from == "name" && to == "title"
    ));
    assert!(versions[1].steps.iter().any(|step| matches!(
        step,
        VersionStep::AddEnumValue { value, downgrade, .. } if value == "HUGE" && downgrade == "LARGE"
    )));
    assert!(versions[1].steps.iter().any(|step| matches!(
        step,
        VersionStep::Require { member, default, .. } if member == "limit" && default == &serde_json::json!(20)
    )));
}

#[test]
fn a_release_is_read_only_under_the_name_it_was_frozen_as() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("migrations-renamed");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Release v0, renamed to v1.json after it was frozen.
    freeze(&dir, &V1.replace("version \"v1\"", "version \"v0\""));
    std::fs::rename(dir.join("releases/v0.json"), dir.join("releases/v1.json")).unwrap();
    let v2 = V1.replace("version \"v1\"", "version \"v2\"") + "migration \"v2\" from \"v1\" {}\n";
    freeze(&dir, &v2);
    std::fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v2.json\"\n"),
    )
    .unwrap();
    let v3 = v2.replace("version \"v2\"", "version \"v3\"") + "migration \"v3\" from \"v2\" {}\n";
    std::fs::write(dir.join("shop.cru"), v3).unwrap();

    let loaded = load_package(&dir).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let error = crucible_lang::versions(&loaded, &compiled, own).unwrap_err();
    assert!(error.contains("not recorded in crucible.lock"), "{error}");
}

#[test]
fn a_release_is_named_only_by_a_version_a_file_can_carry() {
    for version in ["../x", "2026/09", "a:b", ".hidden", ""] {
        assert!(
            crucible_lang::release::file_name(version).is_err(),
            "{version}"
        );
    }
    assert_eq!(
        crucible_lang::release::file_name("2026-09-04").unwrap(),
        "2026-09-04.json"
    );
}

#[test]
fn a_freeze_that_stopped_before_the_lock_is_finished_by_the_next() {
    let dir = package("refreeze", V1, "");
    // As if the first freeze stopped after writing the snapshot.
    std::fs::remove_file(dir.join("releases/crucible.lock")).unwrap();
    freeze(&dir, V1);
    let loaded = load_package(&dir).unwrap();
    let compiled = compile(&loaded, None);
    let guarded =
        guard(&loaded, &compiled, loaded.files.len(), &Config::default()).expect("a baseline");
    assert!(guarded.is_ok(), "{:?}", guarded.err());
}

#[test]
fn a_release_the_lock_records_is_never_refrozen_differently() {
    let dir = package("relocked", V1, "");
    std::fs::remove_file(dir.join("releases/v1.json")).unwrap();
    // The same contract restores the lost file; another is refused.
    freeze(&dir, V1);
    std::fs::remove_file(dir.join("releases/v1.json")).unwrap();
    let staging = dir.with_extension("other");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("crucible.toml"), MANIFEST).unwrap();
    std::fs::write(
        staging.join("shop.cru"),
        V1.replace(
            "        name: string
",
            "        name: integer
",
        ),
    )
    .unwrap();
    let compiled = compile(&load_package(&staging).unwrap(), None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let error =
        crucible_lang::release::freeze(&dir.join("releases"), &compiled.service.unwrap(), "test")
            .unwrap_err();
    assert!(error.contains("with other contents"), "{error}");
}

#[test]
fn an_edited_release_is_refused() {
    let dir = package("edited", V1, "");
    let path = dir.join("releases/v1.json");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("\"v1\"", "\"v1 \"")).unwrap();
    let loaded = load_package(&dir).unwrap();
    let compiled = compile(&loaded, None);
    let Err(error) =
        guard(&loaded, &compiled, loaded.files.len(), &Config::default()).expect("a baseline")
    else {
        panic!("the edited release is refused");
    };
    assert!(error.contains("changed after it was frozen"), "{error}");
}

#[test]
fn a_hooked_requirement_named_by_its_shape_reaches_its_operations() {
    use crucible_smithy::ir::VersionStep;

    let current = V1.replace("        limit?: integer", "        limit: integer");
    let dir = package(
        "require-hook-shape",
        &current,
        "migration \"v2\" from \"v1\" {
    require ListItemsInput.limit by hook
}
",
    );
    let loaded = load_package(&dir).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let versions = crucible_lang::versions(&loaded, &compiled, own).unwrap();
    assert!(
        matches!(
            &versions[0].steps[..],
            [VersionStep::Hooked { operations, .. }] if operations.iter().any(|op| op.name() == "ListItems")
        ),
        "{:?}",
        versions[0].steps
    );
}

#[test]
fn a_sunset_is_a_date_that_exists() {
    let guard = run(
        "sunset-date",
        V1,
        "migration \"v2\" from \"v1\" {\n    sunset \"2027-02-30\"\n}\n",
    );
    assert!(
        problems(&guard).iter().any(|p| p.contains("YYYY-MM-DD")),
        "{:?}",
        problems(&guard)
    );
}

#[test]
fn an_optional_member_cannot_fill_a_required_one() {
    let current = V1.replace(
        "    input { id: string }\n    output {\n        name",
        "    input { id: string }\n    errors [ItemGoneException]\n    output {\n        name",
    ) + "\nerror ItemGoneException {\n    status 410\n    message?: string\n}\n";
    let guard = run(
        "optional-source",
        &current,
        "migration \"v2\" from \"v1\" {\n    add GetItem.errors ItemGoneException downgrade InternalServerException {\n        message <- message\n    }\n}\n",
    );
    assert!(
        problems(&guard)
            .iter()
            .any(|p| p.contains("`message` is optional on ItemGoneException")),
        "{:?}",
        problems(&guard)
    );
}

#[test]
fn every_new_release_has_a_migration() {
    // A release that breaks nothing still declares one, so it stays in the
    // chain of releases the server serves, which its clients name.
    let additive = V1.replace("version \"v1\"", "version \"v2\"");
    let guard = run("no-migration", &additive, "");
    assert!(
        problems(&guard)
            .iter()
            .any(|p| p.contains("no migration comes from the baseline \"v1\"")),
        "{:?}",
        problems(&guard)
    );
    let guard = run(
        "empty-migration",
        &additive,
        "migration \"v2\" from \"v1\" {}
",
    );
    assert!(
        !guard.has_errors(),
        "{:?} {:?}",
        rules(&guard),
        problems(&guard)
    );
}

#[test]
fn a_release_checked_out_with_crlf_line_endings_is_not_an_edit() {
    let dir = package("crlf", V1, "");
    let path = dir.join("releases/v1.json");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace('\n', "\r\n")).unwrap();
    let loaded = load_package(&dir).unwrap();
    let compiled = compile(&loaded, None);
    let guarded =
        guard(&loaded, &compiled, loaded.files.len(), &Config::default()).expect("a baseline");
    assert!(guarded.is_ok(), "{:?}", guarded.err());
}
