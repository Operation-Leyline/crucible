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
        ("null", "integer", "", "null", "must not be null"),
        // `require` makes the member required, so even a document's null,
        // a value where the member is optional, is refused at the top level
        // as it is nested.
        ("null-document", "document", "", "null", "must not be null"),
        // A symbol key and a quoted key that name one wire value would
        // drop a value unseen.
        (
            "enum-map-key-collision",
            "Shades",
            "\nenum Color { RED = \"r\", SPECIAL = \"RED\" }\nmap Shades: Color -> string\n",
            r#"{ RED: "x", "r": "y" }"#,
            "`r` is given twice",
        ),
        // So are two that spell the same key, symbol or not.
        (
            "enum-map-key-repeated",
            "Shades",
            "\nenum Color { RED = \"r\", SPECIAL = \"RED\" }\nmap Shades: Color -> string\n",
            r#"{ RED: "x", "RED": "y" }"#,
            "`RED` is given twice",
        ),
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
        // An optional document may be null; a required one may not.
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
        let error =
            crucible_lang::versions(&loaded, &compiled, loaded.files.len(), &Config::default())
                .into_result(&compiled)
                .unwrap_err();
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
        // Integer enum symbols resolve at depth too, to their numbers.
        (
            "nested-int-enum",
            "Gauge",
            "\nenum Level: integer { LOW = 1, HIGH = 2 }\nlist Levels: Level\nstructure Gauge {\n    level: Level\n    levels?: Levels\n}\n",
            "{ level: HIGH, levels: [LOW, 2] }",
            serde_json::json!({"level": 2, "levels": [1, 2]}),
        ),
        // A union's member is resolved against its target.
        (
            "union-enum",
            "Choice",
            "\nenum Color { RED = \"r\", SPECIAL = \"RED\" }\nstructure Paint { color: Color }\nunion Choice {\n    color: Color\n    paint: Paint\n}\n",
            "{ color: RED }",
            serde_json::json!({"color": "r"}),
        ),
        // Each structure in a list is resolved against the list's member.
        (
            "list-of-structures",
            "Paints",
            "\nenum Color { RED = \"r\", SPECIAL = \"RED\" }\nstructure Paint { color: Color }\nlist Paints: Paint\n",
            r#"[{ color: RED }, { color: "RED" }]"#,
            serde_json::json!([{"color": "r"}, {"color": "RED"}]),
        ),
        // An unquoted key of a map keyed by an enum names a symbol; a quoted
        // key is the wire value itself.
        (
            "enum-map-key",
            "Shades",
            "\nenum Color { RED = \"r\", SPECIAL = \"RED\" }\nmap Shades: Color -> string\n",
            r#"{ RED: "x", SPECIAL: "y" }"#,
            serde_json::json!({"r": "x", "RED": "y"}),
        ),
        (
            "enum-map-key-quoted",
            "Shades",
            "\nenum Color { RED = \"r\", SPECIAL = \"RED\" }\nmap Shades: Color -> string\n",
            r#"{ "RED": "y" }"#,
            serde_json::json!({"RED": "y"}),
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
        let versions =
            crucible_lang::versions(&loaded, &compiled, loaded.files.len(), &Config::default())
                .into_result(&compiled)
                .unwrap()
                .0;
        assert!(
            matches!(
                &versions[0].steps[..],
                [crucible_ir::ir::VersionStep::Require { default, .. }] if default == &expected
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
        let versions =
            crucible_lang::versions(&loaded, &compiled, loaded.files.len(), &Config::default())
                .into_result(&compiled)
                .unwrap()
                .0;
        assert!(
            matches!(
                &versions[0].steps[..],
                [crucible_ir::ir::VersionStep::Require { default, .. }] if default == &expected
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
    let versions =
        crucible_lang::versions(&loaded, &compiled, loaded.files.len(), &Config::default())
            .into_result(&compiled)
            .unwrap()
            .0;
    assert!(matches!(
        &versions[0].steps[..],
        [crucible_ir::ir::VersionStep::Require { default, .. }] if default == &serde_json::json!(1)
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
    let error = crucible_lang::versions(&loaded, &compiled, loaded.files.len(), &Config::default())
        .into_result(&compiled)
        .unwrap_err();
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
    use crucible_ir::ir::VersionStep;

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

    let versions = crucible_lang::versions(&loaded, &compiled, own, &Config::default())
        .into_result(&compiled)
        .unwrap()
        .0;
    let names: Vec<&str> = versions.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["v1", "v2"]);
    assert_eq!(versions[0].sunset.as_deref(), Some("2027-01-01"));
    assert_eq!(versions[0].release.version, "v1");
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
    let error = crucible_lang::versions(&loaded, &compiled, own, &Config::default())
        .into_result(&compiled)
        .unwrap_err();
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
    use crucible_ir::ir::VersionStep;

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
    let versions = crucible_lang::versions(&loaded, &compiled, own, &Config::default())
        .into_result(&compiled)
        .unwrap()
        .0;
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

/// V1's `ListItems`, paged by `nextToken`.
const PAGED: &str = r#"readonly operation ListItems {
    http GET "/items"
    paginated(inputToken: nextToken, outputToken: nextToken, pageSize: limit)
    input {
        @query
        limit?: integer
        @query
        nextToken?: string
    }
    output {
        names: Names
        nextToken?: string
    }
}"#;

const LIST_ITEMS: &str = r#"readonly operation ListItems {
    http GET "/items"
    input {
        @query
        limit?: integer
    }
    output { names: Names }
}"#;

#[test]
fn added_pagination_requires_an_exception_in_the_guard_and_projection() {
    let current = V1.replace(LIST_ITEMS, PAGED);
    assert_ne!(current, V1);
    for (name, steps) in [
        ("pagination-added", ""),
        (
            "pagination-added-behavior",
            "behavior ListItems.pagesResults",
        ),
    ] {
        let dir = package(
            name,
            &current,
            &format!("migration \"v2\" from \"v1\" {{ {steps} }}\n"),
        );
        let loaded = load_package(&dir).unwrap();
        let compiled = compile(&loaded, None);
        assert!(compiled.service.is_some(), "{}", compiled.render());
        let checked = guard(&loaded, &compiled, loaded.files.len(), &Config::default())
            .unwrap()
            .unwrap();
        assert!(checked.has_errors(), "{name}");
        assert!(checked.problems.is_empty(), "{:?}", problems(&checked));
        assert_eq!(
            rules(&checked),
            vec!["compatibility.pagination example.shop#ListItems"],
            "{name}",
        );
        let releases = crucible_lang::Releases::default();
        let versions = crucible_lang::versions_in(
            &loaded,
            &compiled,
            loaded.files.len(),
            &Config::default(),
            &releases,
        );
        assert!(versions.has_errors(), "{name}");
        assert!(
            versions
                .render(&compiled)
                .contains("compatibility.pagination")
        );

        // This is the package manifest's escape hatch, shared by both checks.
        let manifest = dir.join("crucible.toml");
        let text = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(
            &manifest,
            format!("{text}\n[[exceptions]]\nrule = 'compatibility.pagination'\nshape = 'example.shop#ListItems'\nreason = 'Callers explicitly accept paged results'\n"),
        )
        .unwrap();
        let config = Config::load(&manifest).unwrap();
        let checked =
            crucible_lang::check_in(&loaded, &compiled, loaded.files.len(), &config, &releases);
        let guarded = checked.guard.unwrap().unwrap();
        assert!(!guarded.has_errors(), "{}", guarded.render(&compiled));
        assert!(
            !checked.versions.has_errors(),
            "{}",
            checked.versions.render(&compiled)
        );
    }
}

#[test]
fn freezing_a_pagination_addition_does_not_bypass_the_older_release_check() {
    let v2 = V1
        .replace(LIST_ITEMS, PAGED)
        .replace("version \"v1\"", "version \"v2\"");
    let dir = package("pagination-added-in-history", &v2, "");
    freeze(&dir, &v2);
    let manifest = dir.join("crucible.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace("v1.json", "v2.json");
    std::fs::write(&manifest, text).unwrap();
    std::fs::write(
        dir.join("shop.cru"),
        format!(
            "{}\nmigration \"v2\" from \"v1\" {{}}\nmigration \"v3\" from \"v2\" {{}}\n",
            v2.replace("version \"v2\"", "version \"v3\"")
        ),
    )
    .unwrap();
    let loaded = load_package(&dir).unwrap();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let checked = crucible_lang::check_in(
        &loaded,
        &compiled,
        loaded.files.len(),
        &Config::default(),
        &crucible_lang::Releases::default(),
    );
    let guarded = checked.guard.unwrap().unwrap();
    assert!(!guarded.has_errors(), "{}", guarded.render(&compiled));
    assert!(checked.versions.has_errors());
    let rendered = checked.versions.render(&compiled);
    assert!(rendered.contains("compatibility.pagination"), "{rendered}");
    assert!(rendered.contains("v1"), "{rendered}");
}

/// Renaming the page token changes the pagination contract, and the rename
/// steps are what declare and convert it: no policy exception is needed.
/// A change the renames do not explain is still reported.
#[test]
fn renaming_a_page_token_declares_the_pagination_change() {
    let baseline = V1.replace(LIST_ITEMS, PAGED);
    assert_ne!(baseline, V1);
    let renamed = baseline.replace("nextToken", "pageToken");
    let steps = "migration \"v2\" from \"v1\" {\n    rename ListItems.input.nextToken -> pageToken\n    rename ListItems.output.nextToken -> pageToken\n}\n";
    for (name, current, expected) in [
        ("page-token-renamed", renamed.clone(), Vec::<String>::new()),
        (
            "page-token-renamed-and-items-added",
            renamed.replace("pageSize: limit)", "pageSize: limit, items: names)"),
            vec!["compatibility.pagination example.shop#ListItems".to_owned()],
        ),
    ] {
        let dir = package_from(name, &baseline, &current, steps);
        let loaded = load_package(&dir).unwrap();
        let compiled = compile(&loaded, None);
        assert!(compiled.service.is_some(), "{name}: {}", compiled.render());
        let checked = guard(&loaded, &compiled, loaded.files.len(), &Config::default())
            .unwrap()
            .unwrap();
        assert!(
            checked.problems.is_empty(),
            "{name}: {:?}",
            problems(&checked)
        );
        assert_eq!(rules(&checked), expected, "{name}");
    }
}

/// `V1` with an operation whose input has an optional body member.
fn with_create(release: &str) -> String {
    release.replace(
        "operations [GetItem, ListItems, DeleteItem]",
        "operations [GetItem, ListItems, DeleteItem, CreateItem]",
    ) + "\noperation CreateItem {\n    http POST \"/items\"\n    input {\n        name: string\n        description?: string\n    }\n    output { id: string }\n}\n"
}

/// The guard for `current` against `with_create(V1)`, with `steps` as the
/// migration to v2.
fn run_renamed(name: &str, current: &str, steps: &str) -> Guard {
    let dir = package_from(
        name,
        &with_create(V1),
        current,
        &format!("migration \"v2\" from \"v1\" {{\n{steps}}}\n"),
    );
    let loaded = load_package(&dir).unwrap();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{name}: {}", compiled.render());
    guard(&loaded, &compiled, loaded.files.len(), &Config::default())
        .unwrap()
        .unwrap()
}

const RENAME_DESCRIPTION: &str = "    rename CreateItem.input.description -> notes\n";

#[test]
fn a_rename_declares_the_name_and_nothing_else() {
    let renamed = with_create(V1).replace("description?: string", "notes?: string");
    let plain = run_renamed("rename-only", &renamed, RENAME_DESCRIPTION);
    assert_eq!(
        rules(&plain),
        Vec::<String>::new(),
        "{:?}",
        problems(&plain)
    );
    assert_eq!(problems(&plain), Vec::<String>::new());

    // Made required along with the rename: release 1 callers that omit it
    // would be refused, and the rename does not declare that.
    let required = run_renamed(
        "rename-required",
        &renamed.replace("notes?: string", "notes: string"),
        RENAME_DESCRIPTION,
    );
    assert!(required.has_errors());
    assert!(
        rules(&required)
            .contains(&"compatibility.nullability example.shop#CreateItemInput$description".into()),
        "{:?}",
        rules(&required)
    );

    // Retyped along with the rename.
    let retyped = run_renamed(
        "rename-retyped",
        &renamed.replace("notes?: string", "notes?: integer"),
        RENAME_DESCRIPTION,
    );
    assert!(retyped.has_errors());
    assert!(
        rules(&retyped)
            .iter()
            .any(|rule| rule.starts_with("compatibility.type")),
        "{:?}",
        rules(&retyped)
    );

    // An output member that loses its non-null guarantee as it is renamed.
    let output = run_renamed(
        "rename-output-optional",
        &with_create(V1).replace("        name: string\n", "        title?: string\n"),
        "    rename GetItem.output.name -> title\n",
    );
    assert!(
        rules(&output)
            .contains(&"compatibility.nullability example.shop#GetItemOutput$name".into()),
        "{:?}",
        rules(&output)
    );
}

#[test]
fn a_renamed_member_can_also_be_required_or_retyped() {
    let renamed = with_create(V1).replace("description?: string", "notes?: string");

    let required = run_renamed(
        "rename-require",
        &renamed.replace("notes?: string", "notes: string"),
        &format!("{RENAME_DESCRIPTION}    require CreateItem.input.notes default \"\"\n"),
    );
    assert_eq!(
        rules(&required),
        Vec::<String>::new(),
        "{:?}",
        problems(&required)
    );
    assert_eq!(problems(&required), Vec::<String>::new());

    let retyped = run_renamed(
        "rename-retype",
        &renamed.replace("notes?: string", "notes?: integer"),
        &format!("{RENAME_DESCRIPTION}    retype CreateItem.input.notes -> integer by hook\n"),
    );
    assert_eq!(
        rules(&retyped),
        Vec::<String>::new(),
        "{:?}",
        problems(&retyped)
    );
    assert_eq!(problems(&retyped), Vec::<String>::new());

    // The steps name the member as it is now, and a hooked requirement
    // cannot follow a rename.
    for (name, steps, expected) in [
        (
            "rename-require-old-name",
            format!("{RENAME_DESCRIPTION}    require CreateItem.input.description default \"\"\n"),
            "`description` is renamed to `notes` in this migration",
        ),
        (
            "rename-require-hook",
            format!("{RENAME_DESCRIPTION}    require CreateItem.input.notes by hook\n"),
            "a hooked requirement cannot follow yet",
        ),
    ] {
        let guard = run_renamed(
            name,
            &renamed.replace("notes?: string", "notes: string"),
            &steps,
        );
        assert!(
            problems(&guard).iter().any(|p| p.contains(expected)),
            "{name}: {:?}",
            problems(&guard)
        );
    }
}

#[test]
fn a_renamed_member_lowers_its_other_steps_under_both_names() {
    use crucible_ir::ir::{Hook, VersionStep};

    let current = with_create(V1).replace("description?: string", "notes: integer");
    let dir = package_from(
        "rename-lowered",
        &with_create(V1),
        &current,
        &format!(
            "migration \"v2\" from \"v1\" {{\n{RENAME_DESCRIPTION}    require CreateItem.input.notes default 0\n    retype CreateItem.input.notes -> integer by hook\n}}\n"
        ),
    );
    let loaded = load_package(&dir).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let checked = guard(&loaded, &compiled, own, &Config::default())
        .unwrap()
        .unwrap();
    assert!(
        !checked.has_errors(),
        "{:?} {:?}",
        rules(&checked),
        problems(&checked)
    );
    let versions = crucible_lang::versions(&loaded, &compiled, own, &Config::default())
        .into_result(&compiled)
        .unwrap()
        .0;
    let steps = &versions[0].steps;
    assert!(steps.iter().any(|step| matches!(
        step,
        VersionStep::Rename { from, to, .. } if from == "description" && to == "notes"
    )));
    assert!(steps.iter().any(|step| matches!(
        step,
        VersionStep::Require { member, default, .. } if member == "notes" && default == &serde_json::json!(0)
    )));
    assert!(steps.iter().any(|step| matches!(
        step,
        VersionStep::Hooked { hook: Hook::Retype { member, from, .. }, .. }
            if member == "notes" && from == "description"
    )));
}

#[test]
fn a_renamed_member_is_compared_with_its_new_self_wherever_it_is_bound() {
    // Release 1 reads `description` from the query key `q`; release 2 reads
    // the renamed `notes` from `r` and gives `q` to a new member.
    let baseline = with_create(V1).replace(
        "        description?: string\n",
        "        @query(\"q\")\n        description?: string\n",
    );
    let current = |notes: &str| {
        baseline.replace(
            "        @query(\"q\")\n        description?: string\n",
            &format!(
                "        @query(\"r\")\n        {notes}\n        @query(\"q\")\n        filter?: string\n"
            ),
        )
    };
    let run = |name: &str, notes: &str| {
        let dir = package_from(
            name,
            &baseline,
            &current(notes),
            &format!("migration \"v2\" from \"v1\" {{\n{RENAME_DESCRIPTION}}}\n"),
        );
        let loaded = load_package(&dir).unwrap();
        let compiled = compile(&loaded, None);
        assert!(compiled.service.is_some(), "{name}: {}", compiled.render());
        guard(&loaded, &compiled, loaded.files.len(), &Config::default())
            .unwrap()
            .unwrap()
    };

    let optional = run("rename-rebound", "notes?: string");
    assert!(
        !rules(&optional)
            .iter()
            .any(|rule| rule.starts_with("compatibility.nullability")),
        "{:?}",
        rules(&optional)
    );

    // Made required along with the rename: reported against `notes`, not
    // hidden by the member that took over `q`.
    let required = run("rename-rebound-required", "notes: string");
    assert!(
        rules(&required)
            .contains(&"compatibility.nullability example.shop#CreateItemInput$description".into()),
        "{:?}",
        rules(&required)
    );
}

/// The shared test package `tests/models/<name>`.
fn model(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

#[test]
fn every_step_kind_lowers_with_both_sides_and_its_documentation() {
    use crucible_ir::ShapeId;
    use crucible_ir::ir::{Hook, RetypeBy, VersionStep};

    let loaded = load_package(&model("version-steps")).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let checked = guard(&loaded, &compiled, own, &Config::default())
        .unwrap()
        .unwrap();
    assert!(
        !checked.has_errors(),
        "{:?} {:?}",
        rules(&checked),
        problems(&checked)
    );
    let versions = crucible_lang::versions(&loaded, &compiled, own, &Config::default())
        .into_result(&compiled)
        .unwrap()
        .0;
    let [v1] = &versions[..] else {
        panic!("one older release: {versions:?}");
    };
    assert_eq!(v1.name, "v1");
    assert_eq!(v1.sunset.as_deref(), Some("2027-01-01"));
    assert_eq!(
        v1.sunset_docs.as_deref(),
        Some("v1 is served until the end of 2026.")
    );
    assert_eq!(
        v1.docs.as_deref(),
        Some(
            "v2 names an item's title, measures its size as an area, and counts\nitems in [ListItems]."
        )
    );

    let id = |name: &str| format!("example.steps#{name}").parse::<ShapeId>().unwrap();
    // Their doc links resolve in the current model, as any other's do.
    let links = &compiled.service.as_ref().unwrap().doc_links;
    assert_eq!(links.get("ListItems"), Some(&id("ListItems")));
    assert_eq!(
        links.get("GetItem.title"),
        Some(&id("GetItemOutput").with_member("title"))
    );
    // What only the release the migration comes from has, a member's older
    // name and a removed operation, is accepted as text: it links nothing.
    assert_eq!(links.get("GetItem.name"), None);
    assert_eq!(links.get("CountItems"), None);
    let docs = |text: &str| Some(text.to_owned());
    let steps = &v1.steps;
    // One step each, in declaration order; `sunset` is the release's.
    assert_eq!(steps.len(), 13, "{steps:#?}");
    assert_eq!(
        steps[0],
        VersionStep::Rename {
            old: id("GetItemOutput"),
            new: id("GetItemOutput"),
            from: "name".into(),
            to: "title".into(),
            docs: docs("An item's name, [GetItem.name] in v1, is its [GetItem.title]."),
        }
    );
    assert_eq!(
        steps[1],
        VersionStep::Rename {
            old: id("ListItemsInput"),
            new: id("ListItemsInput"),
            from: "max".into(),
            to: "limit".into(),
            docs: docs("The page size is `limit`."),
        }
    );
    // Named as it is now, `limit`; v1 called it `max`.
    assert_eq!(
        steps[2],
        VersionStep::Require {
            old: id("ListItemsInput"),
            new: id("ListItemsInput"),
            member: "limit".into(),
            from: "max".into(),
            default: serde_json::json!(20),
            docs: docs("An older request without a page size gets 20."),
        }
    );
    assert_eq!(
        steps[3],
        VersionStep::Hooked {
            kind: "require".into(),
            operations: vec![id("ListItems")],
            hook: Hook::Require {
                old: id("ListItemsInput"),
                new: id("ListItemsInput"),
                member: "sort".into(),
                from: String::new(),
            },
            is_async: true,
            docs: docs("An older request's sort order is the account's."),
        }
    );
    assert_eq!(
        steps[4],
        VersionStep::AddEnumValue {
            old: id("Kind"),
            new: id("Kind"),
            value: "HUGE".into(),
            downgrade: "LARGE".into(),
            docs: docs("Older clients see a huge item as large."),
        }
    );
    assert_eq!(
        steps[5],
        VersionStep::AddError {
            operation: id("GetItem"),
            error: id("ItemGone"),
            downgrade: id("ItemNotFound"),
            members: vec![("message".into(), "message".into())],
            docs: docs("Older clients see a deleted item as not found."),
        }
    );
    // The older route, which v1 has and v2 does not.
    assert!(
        matches!(
            &steps[6],
            VersionStep::Move { operation, old_method, old_uri, docs }
                if operation == &id("GetItem")
                    && old_method == "GET"
                    && old_uri.to_string() == "/items/{id}"
                    && docs.as_deref() == Some("Items are things now.")
        ),
        "{:#?}",
        steps[6]
    );
    assert_eq!(
        steps[7],
        VersionStep::Hooked {
            kind: "reshape".into(),
            operations: vec![id("GetItem")],
            hook: Hook::Reshape {
                old: id("Size"),
                new: id("Size"),
                members: vec!["width".into(), "height".into()],
                into: "area".into(),
            },
            is_async: false,
            docs: docs("A size is its area."),
        }
    );
    assert_eq!(
        steps[8].docs(),
        Some("The count is a quantity, and a string.")
    );
    // Named as it is now, `quantity`; v1 called it `count`. The documentation
    // above the rename is the rename's, so the retype has none.
    assert_eq!(
        steps[9],
        VersionStep::Hooked {
            kind: "retype".into(),
            operations: vec![id("PutItem")],
            hook: Hook::Retype {
                old: id("PutItemInput"),
                new: id("PutItemInput"),
                member: "quantity".into(),
                from: "count".into(),
                by: RetypeBy::Hook,
            },
            is_async: false,
            docs: None,
        }
    );
    // Without `by hook`: the same two sides, converted by a declared
    // `conversion`.
    assert_eq!(
        steps[10],
        VersionStep::Hooked {
            kind: "retype".into(),
            operations: vec![id("PutItem")],
            hook: Hook::Retype {
                old: id("PutItemOutput"),
                new: id("PutItemOutput"),
                member: "total".into(),
                from: String::new(),
                by: RetypeBy::Conversion,
            },
            is_async: false,
            docs: docs("The total is a string, by a declared conversion."),
        }
    );
    assert_eq!(
        steps[11],
        VersionStep::Hooked {
            kind: "remove operation".into(),
            operations: vec![id("CountItems")],
            hook: Hook::RemoveOperation {
                operation: id("CountItems"),
            },
            is_async: true,
            docs: docs("[CountItems] is gone: ListItems counts items now."),
        }
    );
    assert_eq!(
        steps[12],
        VersionStep::Behavior {
            operation: id("ListItems"),
            name: "ordering".into(),
            docs: docs("ListItems sorts by name."),
        }
    );

    // The steps read back as they were written: every field a step leaves
    // out when empty (`from`, `by: Hook`, `docs`) has a default.
    let written = serde_json::to_value(&v1.steps).unwrap();
    let read: Vec<VersionStep> = serde_json::from_value(written).unwrap();
    assert_eq!(&read, steps);

    // Steps never reach a release: a service's snapshot is the same with its
    // versions as without them, so carrying both sides changes no format.
    let mut service = compiled.service.clone().unwrap();
    let plain = crucible_ir::snapshot::write(&service, "test");
    service.versions = versions.clone();
    assert_eq!(crucible_ir::snapshot::write(&service, "test"), plain);
}

#[test]
fn a_hooked_requirement_on_a_retyped_member_is_refused_at_the_requirement() {
    let loaded = load_package(&model("version-require-retyped")).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let checked = guard(&loaded, &compiled, own, &Config::default())
        .unwrap()
        .unwrap();
    assert!(checked.has_errors());
    let [problem] = &checked.problems[..] else {
        panic!("one problem: {:?}", problems(&checked));
    };
    assert_eq!(
        problem.diagnostic.message,
        "`PutItemInput.note` is both required by hook and retyped in this migration; declare one step for it"
    );
    // At the requirement's path.
    let file = &compiled.files[problem.file.expect("a problem with a line")];
    let span = problem.diagnostic.span;
    assert_eq!(
        &file.text[span.start as usize..span.end as usize],
        "PutItemInput.note"
    );
    let lines = crucible_lang::span::LineIndex::new(&file.text);
    assert_eq!(lines.line_col(&file.text, span.start), (21, 13));
    // The server's versions are refused too, before any generator sees them.
    let error = crucible_lang::versions(&loaded, &compiled, own, &Config::default())
        .into_result(&compiled)
        .unwrap_err();
    assert!(
        error.contains("both required by hook and retyped"),
        "{error}"
    );

    // Only a hook is refused: the same member required by a default, which
    // runs after the retype, is accepted.
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("migrations-require-retyped-default");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("releases")).unwrap();
    let source = model("version-require-retyped");
    for file in [
        "crucible.toml",
        "releases/v1.json",
        "releases/crucible.lock",
    ] {
        std::fs::copy(source.join(file), dir.join(file)).unwrap();
    }
    let text = std::fs::read_to_string(source.join("shop.cru")).unwrap();
    std::fs::write(
        dir.join("shop.cru"),
        text.replace(
            "require PutItemInput.note by hook",
            "require PutItemInput.note default \"\"",
        ),
    )
    .unwrap();
    let loaded = load_package(&dir).unwrap();
    let compiled = compile(&loaded, None);
    let checked = guard(&loaded, &compiled, loaded.files.len(), &Config::default())
        .unwrap()
        .unwrap();
    assert!(
        !checked.has_errors(),
        "{:?} {:?}",
        rules(&checked),
        problems(&checked)
    );
}

#[test]
fn a_dangling_doc_link_in_a_migration_is_an_error() {
    for (name, migration) in [
        (
            "doc-link-migration",
            "/// See [Gone].\nmigration \"v2\" from \"v1\" {}\n",
        ),
        (
            "doc-link-step",
            "migration \"v2\" from \"v1\" {\n    /// See [Gone].\n    sunset \"2027-01-01\"\n}\n",
        ),
    ] {
        let dir = package(name, V1, migration);
        let compiled = compile(&load_package(&dir).unwrap(), None);
        let rendered = compiled.render();
        assert!(
            compiled.service.is_none()
                && rendered.contains("the doc link `[Gone]` names nothing")
                && rendered.contains("nor in release \"v1\", which this migration comes from"),
            "{name}: {rendered}"
        );
    }
}

/// The manifest policy of the shared package at `dir`, as a build reads it.
fn policy(dir: &Path) -> Config {
    let manifest = dir.join("crucible.toml");
    if Config::declares_policy(&std::fs::read_to_string(&manifest).unwrap()) {
        Config::load(&manifest).unwrap()
    } else {
        Config::default()
    }
}

#[test]
fn an_operation_dropped_without_remove_operation_is_refused_at_the_migration() {
    let dir = model("version-gone-operation");
    let loaded = load_package(&dir).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let config = policy(&dir);
    // The migration that dropped it is an older one, which the diff
    // against the baseline does not see.
    let checked = guard(&loaded, &compiled, own, &config).unwrap().unwrap();
    assert!(rules(&checked).is_empty(), "{:?}", rules(&checked));
    let [problem] = &checked.problems[..] else {
        panic!("one problem: {:?}", problems(&checked));
    };
    assert!(problem.diagnostic.is_error());
    assert_eq!(
        problem.diagnostic.message,
        "CountItems is not in \"v3\" and this migration has no `remove operation CountItems`, so nothing would serve it at \"v2\"; declare `remove operation CountItems` and its handler, or except `compatibility.operation` for example.gone#CountItems to stop serving it there and before"
    );
    // At the `from` of `migration "v3" from "v2"`.
    let file = &compiled.files[problem.file.unwrap()];
    let span = problem.diagnostic.span;
    assert_eq!(&file.text[span.start as usize..span.end as usize], "\"v2\"");
    let lines = crucible_lang::span::LineIndex::new(&file.text);
    assert_eq!(lines.line_col(&file.text, span.start), (16, 21));
    // The versions refuse it too, at the same place: what the guard
    // reports already is left out of what they add.
    let mut versions = crucible_lang::versions(&loaded, &compiled, own, &config);
    assert!(versions.has_errors());
    assert!(
        versions.diagnostics().contains(problem),
        "{}",
        versions.render(&compiled)
    );
    versions.without(&checked);
    assert!(
        versions.diagnostics().is_empty(),
        "{}",
        versions.render(&compiled)
    );
}

#[test]
fn an_operation_dropped_under_an_exception_is_retired_with_a_warning() {
    let dir = model("version-retired-operation");
    let loaded = load_package(&dir).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    let config = policy(&dir);
    let checked = guard(&loaded, &compiled, own, &config).unwrap().unwrap();
    assert!(!checked.has_errors(), "{:?}", problems(&checked));
    assert_eq!(
        problems(&checked),
        [
            "CountItems is not in \"v3\" and this migration has no `remove operation CountItems`; under the exception for `compatibility.operation`, \"v2\" and the releases before it no longer serve it"
        ]
    );
    let versions = crucible_lang::versions(&loaded, &compiled, own, &config)
        .into_result(&compiled)
        .unwrap()
        .0;
    let retired: Vec<String> = versions
        .iter()
        .map(|version| {
            let ids: Vec<String> = version.retired.iter().map(ToString::to_string).collect();
            format!("{}: [{}]", version.name, ids.join(", "))
        })
        .collect();
    assert_eq!(
        retired,
        ["v1: []", "v2: [example.gone#CountItems]"],
        "{retired:?}"
    );
    // The exception is what accepts the drop; a setting that turns the
    // finding off does not (decision 68).
    let error = crucible_lang::versions(&loaded, &compiled, own, &Config::default())
        .into_result(&compiled)
        .unwrap_err();
    assert!(error.contains("so nothing would serve it"), "{error}");
}

#[test]
fn a_renamed_enum_value_declares_the_enums_change() {
    // Its wire value kept, or changed: the server maps a changed one.
    for (name, enumeration) in [
        (
            "renamed-enum-value-kept",
            "enum Kind { TINY = \"SMALL\", LARGE }",
        ),
        ("renamed-enum-value-changed", "enum Kind { TINY, LARGE }"),
    ] {
        let guard = run(
            name,
            &V1.replace("enum Kind { SMALL, LARGE }", enumeration),
            "migration \"v2\" from \"v1\" {\n    rename Kind.SMALL -> TINY\n}\n",
        );
        assert!(
            !guard.has_errors(),
            "{name}: {:?} {:?}",
            rules(&guard),
            problems(&guard)
        );
    }
}

#[test]
fn a_renamed_int_enum_value_keeps_its_number() {
    let baseline = V1.replace(
        "enum Kind { SMALL, LARGE }",
        "enum Kind: integer { SMALL = 1, LARGE = 2 }",
    );
    let dir = package_from(
        "renamed-int-enum-value",
        &baseline,
        &baseline.replace("SMALL = 1", "TINY = 3"),
        "migration \"v2\" from \"v1\" {\n    rename Kind.SMALL -> TINY\n}\n",
    );
    let loaded = load_package(&dir).unwrap();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let checked = guard(&loaded, &compiled, loaded.files.len(), &Config::default())
        .unwrap()
        .unwrap();
    assert!(
        problems(&checked).contains(
            &"renaming the intEnum value `SMALL` to `TINY` changes its number from 1 to 3, which older clients send; keep it 1".to_owned()
        ),
        "{:?}",
        problems(&checked)
    );
    // Its number kept, it is declared.
    let dir = package_from(
        "renamed-int-enum-value-kept",
        &baseline,
        &baseline.replace("SMALL = 1", "TINY = 1"),
        "migration \"v2\" from \"v1\" {
    rename Kind.SMALL -> TINY
}
",
    );
    let loaded = load_package(&dir).unwrap();
    let compiled = compile(&loaded, None);
    let kept = guard(&loaded, &compiled, loaded.files.len(), &Config::default())
        .unwrap()
        .unwrap();
    assert!(
        !kept.has_errors(),
        "{:?} {:?}",
        rules(&kept),
        problems(&kept)
    );
}

/// The shop with `Kind` declared as `kinds` at the baseline and `current`
/// now, migrated by `steps`.
fn enum_guard(name: &str, kinds: &str, current: &str, steps: &str) -> Guard {
    let baseline = V1.replace("enum Kind { SMALL, LARGE }", kinds);
    let dir = package_from(
        name,
        &baseline,
        &baseline.replace(kinds, current),
        &format!("migration \"v2\" from \"v1\" {{\n{steps}}}\n"),
    );
    let loaded = load_package(&dir).unwrap();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{name}: {}", compiled.render());
    guard(&loaded, &compiled, loaded.files.len(), &Config::default())
        .unwrap()
        .unwrap()
}

#[test]
fn an_enums_renames_and_additions_compose_into_one_mapping() {
    // The added HUGE takes the wire value SMALL had, which TINY took over:
    // older clients see TINY as SMALL and HUGE as LARGE, in one lookup.
    let composed = enum_guard(
        "enum-composed",
        "enum Kind { SMALL = \"small\", LARGE = \"large\" }",
        "enum Kind { TINY = \"tiny\", LARGE = \"large\", HUGE = \"small\" }",
        "    rename Kind.SMALL -> TINY\n    add Kind.HUGE downgrade LARGE\n",
    );
    assert!(
        !composed.has_errors(),
        "{:?} {:?}",
        rules(&composed),
        problems(&composed)
    );

    // A newer value two steps send two ways.
    let twice = enum_guard(
        "enum-downgraded-twice",
        "enum Kind { SMALL, LARGE }",
        "enum Kind { TINY, LARGE }",
        "    rename Kind.SMALL -> TINY\n    add Kind.TINY downgrade LARGE\n",
    );
    assert!(
        problems(&twice).contains(
            &"Kind.TINY would reach older clients as both Kind.LARGE and Kind.SMALL; declare one step for it".to_owned()
        ),
        "{:?}",
        problems(&twice)
    );

    // Two older values one newer one: older clients could not tell them
    // apart again.
    let merged = enum_guard(
        "enum-merged",
        "enum Kind { SMALL, LARGE }",
        "enum Kind { LARGE }",
        "    rename Kind.SMALL -> LARGE\n",
    );
    assert!(
        problems(&merged).iter().any(|problem| problem
            == "older clients' Kind.LARGE and Kind.SMALL would both be read as `LARGE`; rename to a value the release did not have"),
        "{:?}",
        problems(&merged)
    );
}

#[test]
fn an_enum_change_no_step_explains_keeps_its_finding() {
    // SMALL is renamed, but MEDIUM is dropped with no step: the enum's
    // finding stands, naming it, as it does beside an added value.
    for (name, current, steps) in [
        (
            "enum-unexplained-rename",
            "enum Kind { TINY, LARGE }",
            "    rename Kind.SMALL -> TINY\n",
        ),
        (
            "enum-unexplained-add",
            "enum Kind { SMALL, LARGE, HUGE }",
            "    add Kind.HUGE downgrade LARGE\n",
        ),
    ] {
        let checked = enum_guard(name, "enum Kind { SMALL, LARGE, MEDIUM }", current, steps);
        let finding = checked
            .undeclared
            .iter()
            .find(|finding| finding.rule == "compatibility.enum")
            .unwrap_or_else(|| panic!("{name}: {:?}", rules(&checked)));
        assert_eq!(finding.shape, "example.shop#Kind", "{name}");
        assert!(
            finding
                .message
                .ends_with("; no step declares: MEDIUM removed"),
            "{name}: {}",
            finding.message
        );
    }
}

#[test]
fn a_rename_across_enum_kinds_is_refused() {
    // `"1"` and `1` read alike as text, not as JSON: a retype.
    let checked = enum_guard(
        "enum-kind-changed",
        "enum Kind { ONE = \"1\", TWO = \"2\" }",
        "enum Kind: integer { UNO = 1, TWO = 2 }",
        "    rename Kind.ONE -> UNO\n",
    );
    assert!(
        problems(&checked).contains(
            &"Kind is an enum in the baseline and an integer enum now, so its values change JSON type; a `rename` cannot convert that".to_owned()
        ),
        "{:?}",
        problems(&checked)
    );
}

#[test]
fn the_guard_and_versions_share_the_releases_they_load() {
    let dir = model("version-steps");
    let loaded = load_package(&dir).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    let releases = crucible_lang::Releases::default();
    let checked = crucible_lang::guard_in(&loaded, &compiled, own, &Config::default(), &releases)
        .unwrap()
        .unwrap();
    assert!(!checked.has_errors(), "{:?}", problems(&checked));
    assert_eq!(releases.len(), 1, "the guard read v1 once");
    let versions =
        crucible_lang::versions_in(&loaded, &compiled, own, &Config::default(), &releases)
            .into_result(&compiled)
            .unwrap()
            .0;
    assert_eq!(versions.len(), 1);
    assert_eq!(releases.len(), 1, "versions read nothing again");
    let path = dir.join("releases/v1.json");
    let first = releases.load(&path).unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &first,
        &releases.load(&path).unwrap()
    ));

    // A cached release still answers to its lock: an edited file is
    // refused, not served from the cache.
    let copy = Path::new(env!("CARGO_TARGET_TMPDIR")).join("migrations-releases-cache");
    let _ = std::fs::remove_dir_all(&copy);
    std::fs::create_dir_all(&copy).unwrap();
    for file in ["v1.json", "crucible.lock"] {
        std::fs::copy(dir.join("releases").join(file), copy.join(file)).unwrap();
    }
    let path = copy.join("v1.json");
    releases.load(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{text} ")).unwrap();
    let error = releases.load(&path).unwrap_err();
    assert!(error.contains("changed after it was frozen"), "{error}");
}

/// #346: a rename follows into the permission clauses that read the member,
/// a `when` condition here, so the guard sees the same clause and reports no
/// authorization change.
#[test]
fn a_rename_follows_into_permission_conditions() {
    let dir = model("version-projection-conditions");
    let loaded = load_package(&dir).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let checked = guard(&loaded, &compiled, own, &policy(&dir))
        .unwrap()
        .unwrap();
    assert_eq!(rules(&checked), Vec::<String>::new());
    assert_eq!(problems(&checked), Vec::<String>::new());
}

/// #348: a `retype` declares the binding change it causes, a payload's kind
/// and media type or a member moving between the body and the payload, so
/// the package needs no exception for it.
#[test]
fn a_retype_declares_the_rebinding_it_causes() {
    let dir = model("version-projection-payload");
    let loaded = load_package(&dir).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let config = policy(&dir);
    assert!(config.exceptions.is_empty());
    let checked = guard(&loaded, &compiled, own, &config).unwrap().unwrap();
    assert_eq!(rules(&checked), Vec::<String>::new());
    assert_eq!(problems(&checked), Vec::<String>::new());
    assert!(checked.declared > 0);
}

#[test]
fn error_documentation_changes_require_no_wire_migration() {
    let baseline = "service Errors { version \"v1\" protocol json errors [Problem] } error Problem { status 400 reason?: Reason } enum Reason { FIRST = \"first\" }";
    let current = baseline
        .replace(
            "protocol json",
            "protocol json errorDocs \"https://docs.example.test/errors\"",
        )
        .replace("reason?:", "@errorReason reason?:");
    let dir = package_from("error-documentation", baseline, &current, "");
    let package = load_package(&dir).unwrap();
    let compiled = compile(&package, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let checked = guard(&package, &compiled, package.files.len(), &Config::default())
        .unwrap()
        .unwrap();
    assert!(!checked.has_errors(), "{}", checked.render(&compiled));
    assert_eq!(checked.declared, 0);
}

#[test]
fn error_retypes_reconcile_reason_designation_with_the_restored_member() {
    for (old_type, declaration, expected_reason) in [
        ("integer", "", None),
        ("OldReason", "enum OldReason { OLD = \"a\" }", Some("a")),
    ] {
        // Keep the older enum reachable in both releases; its retirement
        // would be a separate compatibility change from the member's retype.
        let legacy = if expected_reason.is_some() {
            "legacy?: OldReason"
        } else {
            ""
        };
        let baseline = format!(
            "service Independent {{ version \"v1\" protocol json errors [Problem] operations [Read] }} readonly operation Read {{ http GET \"/read\" output {{}} }} error Problem {{ status 409 message: string detail?: {old_type} {legacy} }} {declaration}"
        );
        let current = baseline
            .replace(
                "protocol json",
                "protocol json errorDocs \"https://docs.test/errors\"",
            )
            .replace(
                &format!("detail?: {old_type}"),
                "@errorReason detail?: Reason",
            )
            + " enum Reason { A = \"a\" }";
        let dir = package_from(
            &format!("error-reason-retype-{old_type}"),
            &baseline,
            &current,
            "migration \"v2\" from \"v1\" { retype Problem.detail -> Reason by hook }",
        );
        let service = crucible_lang::load_versioned_service(&dir)
            .unwrap_or_else(|error| panic!("{old_type}: {error}"));
        let id = crucible_ir::ShapeId::new("example.shop", "Problem");
        let current = service.error_documentation(&id).unwrap();
        assert_eq!(current.reason_member.as_deref(), Some("detail"));
        assert_eq!(current.reasons[0].value, "a");
        let projected = crucible_ir::versions::projections(&service).unwrap();
        let historical = projected[0].service.error_documentation(&id).unwrap();
        assert_eq!(
            historical.url.as_deref(),
            Some("https://docs.test/errors/Problem")
        );
        match expected_reason {
            Some(value) => {
                assert_eq!(historical.reason_member.as_deref(), Some("detail"));
                assert_eq!(historical.reasons[0].value, value);
                assert_eq!(historical.reasons[0].name, "OLD");
                assert_eq!(
                    historical.reasons[0].url.as_deref(),
                    Some("https://docs.test/errors/Problem/reason-a")
                );
            }
            None => {
                assert_eq!(historical.reason_member, None);
                assert!(historical.reasons.is_empty());
            }
        }
    }
}

#[test]
fn historical_error_reason_defaults_come_from_the_restored_member() {
    let baseline = r#"service Independent { version "v1" protocol json errors [Problem] operations [Read] }
        readonly operation Read { http GET "/read" output {} }
        error Problem { status 409 message: string detail: OldReason = EMPTY legacy?: OldReason }
        enum OldReason { EMPTY = "access/needed" }"#;
    let current = baseline
        .replace(
            "protocol json",
            "protocol json errorDocs \"https://docs.test/errors\"",
        )
        .replace(
            "detail: OldReason = EMPTY",
            "@errorReason detail: Reason = ACCESS",
        )
        + " enum Reason { ACCESS = \"access/needed\" }";
    let dir = package_from(
        "error-reason-default-retype",
        baseline,
        &current,
        "migration \"v2\" from \"v1\" { retype Problem.detail -> Reason by hook }",
    );
    let service =
        crucible_lang::load_versioned_service(&dir).unwrap_or_else(|error| panic!("{error}"));
    let id = crucible_ir::ShapeId::new("example.shop", "Problem");
    assert_eq!(
        service
            .error_documentation(&id)
            .unwrap()
            .reason_default
            .as_deref(),
        Some("access/needed")
    );
    let projected = crucible_ir::versions::projections(&service).unwrap();
    assert_eq!(
        projected[0]
            .service
            .error_documentation(&id)
            .unwrap()
            .reason_default
            .as_deref(),
        Some("access/needed")
    );
    // Default-changing source migrations have a separate compatibility.default
    // policy. Direct IR still proves selection does not borrow today's value.
    fn set_default(model: &mut crucible_ir::ir::Service, id: &crucible_ir::ShapeId, value: &str) {
        let crucible_ir::ir::Shape::Structure(shape) = model.shapes.get_mut(id).unwrap() else {
            panic!()
        };
        let member = shape
            .members
            .iter_mut()
            .find(|member| member.name == "detail")
            .unwrap();
        member.default = Some(serde_json::json!(value));
        member.wire_default = Some(serde_json::json!(value));
        let target = member.target.clone();
        let crucible_ir::ir::Shape::Enum(shape) = model.shapes.get_mut(&target).unwrap() else {
            panic!()
        };
        shape.values[0].value = value.into();
    }
    for (old, current) in [("", "access/needed"), ("access/needed", "")] {
        let mut direct = service.clone();
        set_default(&mut direct, &id, current);
        set_default(&mut direct.versions[0].release, &id, old);
        // The retained legacy member keeps the old enum reachable; undo uses
        // that current definition when resolving a restored member's target.
        let old_enum = crucible_ir::ShapeId::new("example.shop", "OldReason");
        let crucible_ir::ir::Shape::Enum(shape) = direct.shapes.get_mut(&old_enum).unwrap() else {
            panic!()
        };
        shape.values[0].value = old.into();
        assert_eq!(
            direct
                .error_documentation(&id)
                .unwrap()
                .reason_default
                .as_deref(),
            Some(current)
        );
        let projected = crucible_ir::versions::projections(&direct).unwrap();
        let historical = projected[0].service.error_documentation(&id).unwrap();
        assert_eq!(historical.reason_default.as_deref(), Some(old));
        let page = historical
            .reasons
            .iter()
            .find(|reason| reason.value == old)
            .unwrap()
            .url
            .as_deref();
        assert_eq!(
            page,
            Some(if old.is_empty() {
                "https://docs.test/errors/Problem/reason-"
            } else {
                "https://docs.test/errors/Problem/reason-access%2Fneeded"
            })
        );
        assert_eq!(
            direct
                .error_documentation(&id)
                .unwrap()
                .reason_default
                .as_deref(),
            Some(current)
        );
    }
}

#[test]
fn reason_designation_survives_three_release_retypes_and_member_renames() {
    for renamed in [false, true] {
        let (old, middle, current) = if renamed {
            ("first", "middle", "detail")
        } else {
            ("detail", "detail", "detail")
        };
        let baseline = format!(
            r#"service Independent {{ version "v1" protocol json errors [Problem] operations [Read] }}
            readonly operation Read {{ http GET "/read" output {{}} }}
            error Problem {{ status 409 {old}?: Reason legacy?: Reason }} enum Reason {{ ACCESS = "access/needed" }}"#
        );
        let rename2 = if renamed {
            "rename Problem.first -> middle"
        } else {
            ""
        };
        let rename3 = if renamed {
            "rename Problem.middle -> detail"
        } else {
            ""
        };
        let to_v2 = format!(
            "migration \"v2\" from \"v1\" {{ {rename2} retype Problem.{middle} -> integer by hook }}"
        );
        let v2 = baseline
            .replace("version \"v1\"", "version \"v2\"")
            .replace(&format!("{old}?: Reason"), &format!("{middle}?: integer"))
            + "\n"
            + &to_v2;
        let to_v3 = format!(
            "migration \"v3\" from \"v2\" {{ {rename3} retype Problem.{current} -> Reason by hook }}"
        );
        let v3 = v2
            .replace("version \"v2\"", "version \"v3\"")
            .replace(
                "protocol json",
                "protocol json errorDocs \"https://docs.test/errors\"",
            )
            .replace(
                &format!("{middle}?: integer"),
                &format!("@errorReason {current}?: Reason"),
            )
            + "\n"
            + &to_v3;
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("error-docs-three-releases-{renamed}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        freeze(&dir, &baseline);
        freeze(&dir, &v2);
        std::fs::write(
            dir.join("crucible.toml"),
            format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v2.json\"\n"),
        )
        .unwrap();
        std::fs::write(dir.join("shop.cru"), v3).unwrap();
        let service =
            crucible_lang::load_versioned_service(&dir).unwrap_or_else(|error| panic!("{error}"));
        let id = crucible_ir::ShapeId::new("example.shop", "Problem");
        assert_eq!(
            service
                .error_documentation(&id)
                .unwrap()
                .reason_member
                .as_deref(),
            Some(current)
        );
        let versions = crucible_ir::versions::projections(&service).unwrap();
        let oldest = versions[0].service.error_documentation(&id).unwrap();
        assert_eq!(oldest.reason_member.as_deref(), Some(old));
        assert_eq!(
            oldest.reasons[0].url.as_deref(),
            Some("https://docs.test/errors/Problem/reason-access%2Fneeded")
        );
        let scalar = versions[1].service.error_documentation(&id).unwrap();
        assert_eq!(scalar.reason_member, None);
        assert!(scalar.reasons.is_empty());
    }
}
