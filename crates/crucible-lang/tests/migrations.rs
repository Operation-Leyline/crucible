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
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("migrations-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    freeze(&dir, V1);
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
