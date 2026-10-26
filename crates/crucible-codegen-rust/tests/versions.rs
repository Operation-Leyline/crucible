//! Older API versions (docs/api-versions.md): the generated `versions`
//! module for the e2e fixture, checked in, and what the generator reports
//! instead of converting. `CRUCIBLE_BLESS=1 cargo test` rewrites the golden.

use std::fs;
use std::path::{Path, PathBuf};

use crucible_codegen_rust::{Options, generate};
use crucible_ir::ir::Service;

/// The package at `dir`, lowered with the versions it serves, as a build
/// lowers it: the compatibility guard must pass first, so every model a
/// test generates is one the compiler accepts.
fn lower(dir: &Path) -> Service {
    let package = crucible_lang::load_package(dir).unwrap();
    let own = package.files.len();
    let compiled = crucible_lang::compile(&package, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let manifest = dir.join("crucible.toml");
    let config =
        if crucible_policy::Config::declares_policy(&fs::read_to_string(&manifest).unwrap()) {
            crucible_policy::Config::load(&manifest).unwrap()
        } else {
            crucible_policy::Config::default()
        };
    if let Some(guard) = crucible_lang::guard(&package, &compiled, own, &config) {
        let guard = guard.unwrap();
        assert!(!guard.has_errors(), "{}", guard.render(&compiled));
    }
    let versions = crucible_lang::versions(&package, &compiled, own, &config)
        .into_result(&compiled)
        .unwrap()
        .0;
    let mut service = compiled.service.expect("the package compiles");
    service.versions = versions;
    service
}

/// [`lower`] for a package the compiler refuses while it projects an older
/// release, with a problem containing `message`: the versions as built,
/// for the generator's own check, which stays as a backstop for an edited
/// IR.
fn lower_refused(dir: &Path, message: &str) -> Service {
    let package = crucible_lang::load_package(dir).unwrap();
    let own = package.files.len();
    let compiled = crucible_lang::compile(&package, None);
    let config = crucible_policy::Config::load(&dir.join("crucible.toml")).unwrap();
    let versions = crucible_lang::versions(&package, &compiled, own, &config);
    let rendered = versions.render(&compiled);
    assert!(
        versions.has_errors() && rendered.contains(message),
        "{rendered}"
    );
    let mut service = compiled.service.expect("the package compiles");
    service.versions = versions.versions;
    service
}

#[test]
fn the_versioned_fixture_matches_golden() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    let code = generate(&lower(&fixture), &Options::default()).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/versioned.rs");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::write(&path, &code).unwrap();
    }
    let golden = fs::read_to_string(&path).expect("golden exists; bless to create it");
    assert!(
        golden.replace("\r\n", "\n") == code,
        "generated code differs from {}; rerun with CRUCIBLE_BLESS=1 and read the diff",
        path.display()
    );
}

const V1: &str = r#"service Shop {
    version "v1"
    protocol json
    operations [GetCounts]
    errors [ValidationException, InternalServerException]
}

readonly operation GetCounts {
    http GET "/counts"
    output { byKind: Counts }
}

map Counts: Kind -> integer

enum Kind { SMALL, LARGE }
"#;

const MANIFEST: &str = "[model]\nnamespace = \"example.shop\"\nimports = [\"crucible.errors\"]\n";

/// The resource tests remove the operation that alone reached some
/// identifier shapes and permissions, a break of the SDK's source they do
/// not exercise: the guard judges only the wire.
const SDK_OFF: &str = "\n[compatibility]\nsdk_source = false\n";

/// The SDK break a shape rename makes, which the setting above hides from
/// the guard but not from the projection check (decision 68).
const SHELF_COUNT: &str = "\n[[exceptions]]\nrule = \"compatibility.sdk\"\nshape = \"example.shop#Shelf$count\"\nreason = \"Shelf is Bookshelf now\"\n";

/// Freezes `source`, a release of the shop, into `dir/releases`, as
/// `crucible freeze` does: compiled once, then a snapshot.
fn freeze(dir: &Path, source: &str) {
    let staging = dir.with_extension("staging");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("crucible.toml"), MANIFEST).unwrap();
    fs::write(staging.join("shop.cru"), source).unwrap();
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&staging).unwrap(), None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let service = compiled.service.unwrap();
    crucible_lang::release::freeze(&dir.join("releases"), &service, "test").unwrap();
}

/// A package whose release v1 is `V1`, at v2 with `current` and `migration`.
fn package(name: &str, current: &str, migration: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("versions-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    freeze(&dir, V1);
    fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v1.json\"\n"),
    )
    .unwrap();
    fs::write(
        dir.join("shop.cru"),
        format!(
            "{}\n{migration}",
            current.replace("version \"v1\"", "version \"v2\"")
        ),
    )
    .unwrap();
    dir
}

#[test]
fn an_enum_value_that_keys_a_map_is_reported() {
    let dir = package(
        "keyed",
        &V1.replace(
            "enum Kind { SMALL, LARGE }",
            "enum Kind { SMALL, LARGE, HUGE }",
        ),
        "migration \"v2\" from \"v1\" {\n    add Kind.HUGE downgrade LARGE\n}\n",
    );
    let diagnostics = generate(&lower(&dir), &Options::default()).unwrap_err();
    assert!(
        diagnostics.to_string().contains("keys a map"),
        "{diagnostics}"
    );
}

#[test]
fn a_retype_without_a_hook_is_reported() {
    let v1 = V1.replace(
        "output { byKind: Counts }",
        "output { byKind: Counts, total: integer }",
    );
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("versions-conversion");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    freeze(&dir, &v1);
    fs::write(
        dir.join("crucible.toml"),
        format!(
            "{MANIFEST}
[model.baseline]
path = \"releases/v1.json\"
"
        ),
    )
    .unwrap();
    let current = v1
        .replace("version \"v1\"", "version \"v2\"")
        .replace("total: integer", "total: string");
    fs::write(
        dir.join("shop.cru"),
        format!(
            "{current}
migration \"v2\" from \"v1\" {{
    retype GetCountsOutput.total -> string
}}
"
        ),
    )
    .unwrap();
    let diagnostics = generate(&lower(&dir), &Options::default()).unwrap_err();
    assert!(
        diagnostics.to_string().contains("declared `conversion`"),
        "{diagnostics}"
    );
}

#[test]
fn a_release_declaring_migration_is_reported_when_it_needs_hooks() {
    let v1 = V1.replace(
        "operations [GetCounts]",
        "operations [GetCounts, Migration]",
    ) + "\nreadonly operation Migration {\n    http GET \"/migration\"\n    output { done: boolean }\n}\n";
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("versions-migration-name");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    freeze(&dir, &v1);
    fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v1.json\"\n"),
    )
    .unwrap();
    let current = v1
        .replace("version \"v1\"", "version \"v2\"")
        .replace("output { done: boolean }", "output { done: string }");
    fs::write(
        dir.join("shop.cru"),
        format!("{current}\nmigration \"v2\" from \"v1\" {{\n    retype MigrationOutput.done -> string by hook\n}}\n"),
    )
    .unwrap();
    let diagnostics = generate(&lower(&dir), &Options::default()).unwrap_err();
    assert!(
        diagnostics.to_string().contains("declares `Migration`"),
        "{diagnostics}"
    );
}

#[test]
fn a_cru_service_without_migrations_still_knows_its_release() {
    // The language's weather example: no migrations, so no older releases,
    // but its server refuses a client of any other release.
    let release = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/idl/examples/weather");
    let code = generate(&lower(&release), &Options::default()).unwrap();
    assert!(
        code.contains(".versions(::crucible_server_runtime_rust::__generated::versioning::ApiVersion::new(0, \"2026-10-01\"), &[])"),
        "{code}"
    );
    assert!(!code.contains("pub mod versions"));
}

/// A legacy handler's checks bind the old input positionally against the
/// current resource description: a resource moved under another parent
/// keeps its identifier count, so a binding would name the wrong instance.
/// Anything but an unchanged identifier list is refused.
#[test]
fn a_legacy_check_on_a_resource_whose_identifiers_changed_is_reported() {
    let v1 = r#"service Board {
    version "v1"
    protocol json
    auth bearer
    resources [Shelf, Folder]
    errors [ValidationException, InternalServerException]
}

string ShelfId
string FolderId
string NoteId

resource Shelf {
    id shelfId: ShelfId
    permission count = "shelf:count"
}

resource Folder {
    id folderId: FolderId
    permission count = "folder:count"
}

resource Note in Shelf {
    id noteId: NoteId
    permission read = "note:read"
}

readonly operation ReadNote reads Note {
    http GET "/shelves/{shelfId}/notes/{noteId}"
    requires read
    input {
        $shelfId
        $noteId
    }
    output { body: string }
}
"#;
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("versions-reparented");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    freeze(&dir, v1);
    fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v1.json\"\n{SDK_OFF}"),
    )
    .unwrap();
    let current = v1
        .replace("version \"v1\"", "version \"v2\"")
        .replace("resource Note in Shelf", "resource Note in Folder");
    let current = current[..current.find("readonly operation ReadNote").unwrap()].to_owned();
    fs::write(
        dir.join("shop.cru"),
        format!("{current}\nmigration \"v2\" from \"v1\" {{\n    remove operation ReadNote\n}}\n"),
    )
    .unwrap();
    let message =
        "whose identifiers have changed since (`(shelfId, noteId)` then, `(folderId, noteId)` now)";
    let diagnostics = generate(&lower_refused(&dir, message), &Options::default()).unwrap_err();
    assert!(diagnostics.to_string().contains(message), "{diagnostics}");
}

/// The same move to a parent whose identifier has the same name keeps the
/// identifier list, so the release's recorded parents tell it apart.
#[test]
fn a_legacy_check_on_a_resource_moved_to_a_same_named_parent_is_reported() {
    let v1 = r#"service Board {
    version "v1"
    protocol json
    auth bearer
    resources [Shelf, Folder]
    errors [ValidationException, InternalServerException]
}

string ShelfId
string NoteId

resource Shelf {
    id shelfId: ShelfId
    permission count = "shelf:count"
}

resource Folder {
    id shelfId: ShelfId
    permission count = "folder:count"
}

resource Note in Shelf {
    id noteId: NoteId
    permission read = "note:read"
}

readonly operation ReadNote reads Note {
    http GET "/shelves/{shelfId}/notes/{noteId}"
    requires read
    input {
        $shelfId
        $noteId
    }
    output { body: string }
}
"#;
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("versions-reparented-same-name");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    freeze(&dir, v1);
    fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v1.json\"\n{SDK_OFF}"),
    )
    .unwrap();
    let current = v1
        .replace("version \"v1\"", "version \"v2\"")
        .replace("resource Note in Shelf", "resource Note in Folder");
    let current = current[..current.find("readonly operation ReadNote").unwrap()].to_owned();
    fs::write(
        dir.join("shop.cru"),
        format!("{current}\nmigration \"v2\" from \"v1\" {{\n    remove operation ReadNote\n}}\n"),
    )
    .unwrap();
    let message = "whose parent resources have changed since (`(shelf)` then, `(folder)` now)";
    let diagnostics = generate(&lower_refused(&dir, message), &Options::default()).unwrap_err();
    assert!(diagnostics.to_string().contains(message), "{diagnostics}");
}

/// A root resource moved under a parent with no identifier keeps its
/// identifier list; its recorded parents (none) tell it apart.
#[test]
fn a_legacy_check_on_a_root_resource_moved_under_a_parent_is_reported() {
    let v1 = r#"service Board {
    version "v1"
    protocol json
    auth bearer
    resources [Note, Folder]
    errors [ValidationException, InternalServerException]
}

string NoteId

resource Folder {
}

resource Note {
    id noteId: NoteId
    permission read = "note:read"
}

readonly operation ReadNote reads Note {
    http GET "/notes/{noteId}"
    requires read
    input { $noteId }
    output { body: string }
}
"#;
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("versions-root-reparented");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    freeze(&dir, v1);
    fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v1.json\"\n{SDK_OFF}"),
    )
    .unwrap();
    let current = v1
        .replace("version \"v1\"", "version \"v2\"")
        .replace("resources [Note, Folder]", "resources [Folder]")
        .replace("resource Note {", "resource Note in Folder {");
    let current = current[..current.find("readonly operation ReadNote").unwrap()].to_owned();
    fs::write(
        dir.join("shop.cru"),
        format!("{current}\nmigration \"v2\" from \"v1\" {{\n    remove operation ReadNote\n}}\n"),
    )
    .unwrap();
    let message = "whose parent resources have changed since (`()` then, `(Folder)` now)";
    let diagnostics = generate(&lower_refused(&dir, message), &Options::default()).unwrap_err();
    assert!(diagnostics.to_string().contains(message), "{diagnostics}");
}

/// Parents are recorded by authorization name, which grants store and a
/// shape rename keeps: renaming the parent's shape is no move.
#[test]
fn a_legacy_check_survives_its_parent_shape_being_renamed() {
    let v1 = r#"service Board {
    version "v1"
    protocol json
    auth bearer
    resources [Shelf]
    errors [ValidationException, InternalServerException]
}

string ShelfId
string NoteId

resource Shelf {
    id shelfId: ShelfId
    permission count = "shelf:count"
}

resource Note in Shelf {
    id noteId: NoteId
    permission read = "note:read"
}

readonly operation ReadNote reads Note {
    http GET "/shelves/{shelfId}/notes/{noteId}"
    requires read
    input {
        $shelfId
        $noteId
    }
    output { body: string }
}
"#;
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("versions-parent-renamed");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    freeze(&dir, v1);
    fs::write(
        dir.join("crucible.toml"),
        format!(
            "{MANIFEST}\n[model.baseline]\npath = \"releases/v1.json\"\n{SDK_OFF}{SHELF_COUNT}"
        ),
    )
    .unwrap();
    let current = v1
        .replace("version \"v1\"", "version \"v2\"")
        .replace("resources [Shelf]", "resources [Bookshelf]")
        .replace(
            "resource Shelf {",
            "@authorizationName(\"shelf\")\nresource Bookshelf {",
        )
        .replace("resource Note in Shelf", "resource Note in Bookshelf");
    let current = current[..current.find("readonly operation ReadNote").unwrap()].to_owned();
    fs::write(
        dir.join("shop.cru"),
        format!("{current}\nmigration \"v2\" from \"v1\" {{\n    remove operation ReadNote\n}}\n"),
    )
    .unwrap();
    generate(&lower(&dir), &Options::default())
        .expect("a parent shape rename is not a resource move");
}

#[test]
fn a_renamed_member_is_converted_under_both_names() {
    let v1 = r#"service Shop {
    version "v1"
    protocol json
    operations [PutNote]
    errors [ValidationException, InternalServerException]
}

operation PutNote {
    http POST "/notes"
    input {
        description?: string
        size?: string
    }
    output { description?: string, size?: string }
}
"#;
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("versions-renamed");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    freeze(&dir, v1);
    fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v1.json\"\n"),
    )
    .unwrap();
    // `description` becomes a required `notes`; `size` becomes an integer
    // `bytes`.
    let current = v1
        .replace("version \"v1\"", "version \"v2\"")
        .replace("description?: string\n", "notes: string\n")
        .replace("size?: string\n", "bytes?: integer\n")
        .replace(
            "output { description?: string, size?: string }",
            "output { notes?: string, bytes?: integer }",
        );
    fs::write(
        dir.join("shop.cru"),
        format!(
            "{current}
migration \"v2\" from \"v1\" {{
    rename PutNoteInput.description -> notes
    require PutNoteInput.notes default \"\"
    rename PutNoteInput.size -> bytes
    retype PutNoteInput.bytes -> integer by hook
    rename PutNoteOutput.description -> notes
    rename PutNoteOutput.size -> bytes
    retype PutNoteOutput.bytes -> integer by hook
}}
"
        ),
    )
    .unwrap();
    let code = generate(&lower(&dir), &Options::default()).unwrap();
    // A request reads `size` before it is renamed, and the default fills
    // `notes` only when `description` did not arrive to be renamed into it.
    for expected in [
        r#"Transform::Hook { at: &[Seg::Field("size")], hook: "versions::v1::Migration::upgrade_put_note_input_bytes" }"#,
        r#"Transform::Rename { at: &[], from: "description", to: "notes" }"#,
        r#"Transform::Default { at: &[], member: "notes", value: "\"\"" }"#,
        r#"Transform::Rename { at: &[], from: "size", to: "bytes" }"#,
        // A response converts `bytes` before it is renamed back.
        r#"Transform::Hook { at: &[Seg::Field("bytes")], hook: "versions::v1::Migration::downgrade_put_note_output_bytes" }"#,
        r#"Transform::Rename { at: &[], from: "bytes", to: "size" }"#,
    ] {
        assert!(code.contains(expected), "{expected}\n{code}");
    }
    let at = |text: &str| code.find(text).expect(text);
    assert!(at("upgrade_put_note_input_bytes") < at(r#"from: "size", to: "bytes""#));
    assert!(at("downgrade_put_note_output_bytes") < at(r#"from: "bytes", to: "size""#));
}

/// The shared test package `tests/models/<name>`.
fn model(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

/// The diagnostics generating `service` reports, one per line.
fn reported(service: &Service) -> Vec<String> {
    generate(service, &Options::default())
        .expect_err("the generator refuses the model")
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn an_operation_retired_under_an_exception_is_not_served_at_older_versions() {
    let service = lower(&model("version-retired-operation"));
    assert_eq!(
        service.versions[1].retired,
        ["example.gone#CountItems"
            .parse::<crucible_ir::ShapeId>()
            .unwrap()]
    );
    let code = generate(&service, &Options::default()).unwrap();
    assert!(
        code.contains("add_versioned_identity::<v1::GetItem"),
        "{code}"
    );
    for registration in ["add_versioned", "add_versioned_identity"] {
        assert!(
            !code.contains(&format!("{registration}::<v1::CountItems")),
            "{code}"
        );
        assert!(
            !code.contains(&format!("{registration}::<v2::CountItems")),
            "{code}"
        );
    }

    // Without the record of the exception, which only an edited IR lacks,
    // nothing would serve it: reported once, though v1 and v2 both reach
    // the gap.
    let mut edited = service;
    edited.versions[1].retired.clear();
    assert_eq!(
        reported(&edited),
        [
            "example.gone#CountItems: CountItems is not in API version v3, and the migration from v2 neither removes it with `remove operation CountItems` nor retires it; nothing serves it at v2 or before"
        ]
    );
}

#[test]
fn a_renamed_enum_value_is_mapped_both_ways() {
    let code = generate(
        &lower(&model("version-renamed-enum-value")),
        &Options::default(),
    )
    .unwrap();
    for expected in [
        // A request's value, as the next version reads it.
        r#"upgrade: &[Transform::Map { at: &[Seg::Field("like")], values: &[("small", "tiny")] }]"#,
        // A response's value, as older clients read it.
        r#"downgrade: &[Transform::Map { at: &[Seg::Field("kind")], values: &[("tiny", "small")] }]"#,
    ] {
        assert!(code.contains(expected), "{expected}\n{code}");
    }

    // With the wire value kept, nothing on the wire changes: nothing maps.
    let dir = package(
        "renamed-enum-value-kept",
        &V1.replace(
            "enum Kind { SMALL, LARGE }",
            "enum Kind { TINY = \"SMALL\", LARGE }",
        ),
        "migration \"v2\" from \"v1\" {\n    rename Kind.SMALL -> TINY\n}\n",
    );
    let code = generate(&lower(&dir), &Options::default()).unwrap();
    assert!(!code.contains("Transform::Map"), "{code}");

    // A renamed value that keys a map is not converted yet, as an added one
    // is not.
    let dir = package(
        "renamed-enum-value-keyed",
        &V1.replace("enum Kind { SMALL, LARGE }", "enum Kind { TINY, LARGE }"),
        "migration \"v2\" from \"v1\" {\n    rename Kind.SMALL -> TINY\n}\n",
    );
    let reported = reported(&lower(&dir));
    assert!(
        reported
            .iter()
            .any(|line| line.contains("keys a map in example.shop#GetCounts's output")),
        "{reported:#?}"
    );
}

#[test]
fn a_step_its_release_does_not_have_is_reported_not_skipped() {
    use crucible_ir::ir::{Hook, VersionStep};

    let fixture =
        lower(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned"));
    let gone: crucible_ir::ShapeId = "example.shop#Gone".parse().unwrap();
    // Each edit makes one step name what its release does not have, as
    // only an edited IR can: lowering checks every step against both
    // releases. `index` is the older release whose migration it edits.
    type Edit = Box<dyn Fn(&mut VersionStep) -> bool>;
    let cases: Vec<(&str, usize, Edit, &[&str])> = vec![
        (
            "rename, older side",
            0,
            Box::new(|step| match step {
                VersionStep::Rename { from, .. } if from == "name" => {
                    "gone".clone_into(from);
                    true
                }
                _ => false,
            }),
            &[
                "example.shop#GetItemOutput$gone: `rename GetItemOutput.gone -> title`, in the migration from API version v1, names example.shop#GetItemOutput$gone, which API version v1 does not have; GetItem is not converted across it",
            ],
        ),
        (
            "rename, newer side",
            0,
            Box::new(|step| match step {
                VersionStep::Rename { from, to, .. } if from == "name" => {
                    "gone".clone_into(to);
                    true
                }
                _ => false,
            }),
            &[
                "example.shop#GetItemOutput$gone: `rename GetItemOutput.name -> gone`, in the migration from API version v1, names example.shop#GetItemOutput$gone, which API version v2 does not have; GetItem is not converted across it",
            ],
        ),
        (
            "rename, a member become an enum value",
            0,
            Box::new(|step| match step {
                VersionStep::Rename { new, from, to, .. } if from == "name" => {
                    *new = "example.shop#Kind".parse().unwrap();
                    "SMALL".clone_into(to);
                    true
                }
                _ => false,
            }),
            &[
                "example.shop#Kind$SMALL: `rename GetItemOutput.name -> SMALL`, in the migration from API version v1, names a structure member in API version v1 and an enum value in API version v2; GetItem is not converted across it",
                "example.shop#Kind$SMALL: `rename GetItemOutput.name -> SMALL`, in the migration from API version v1, names a structure member in API version v1 and an enum value in API version v2; ListItems is not converted across it",
                "example.shop#Kind$SMALL: `rename GetItemOutput.name -> SMALL`, in the migration from API version v1, names a structure member in API version v1 and an enum value in API version v2; ListLabels is not converted across it",
            ],
        ),
        (
            "require by default",
            1,
            Box::new(|step| match step {
                VersionStep::Require { member, .. } => {
                    "gone".clone_into(member);
                    true
                }
                _ => false,
            }),
            &[
                "example.shop#ListItemsInput$gone: `require ListItemsInput.gone default ..`, in the migration from API version v2, names example.shop#ListItemsInput$gone, which API version v3 does not have; ListItems is not converted across it",
            ],
        ),
        (
            "add enum value, the downgrade",
            1,
            Box::new(|step| match step {
                VersionStep::AddEnumValue { downgrade, .. } => {
                    "GONE".clone_into(downgrade);
                    true
                }
                _ => false,
            }),
            &[
                "example.shop#Kind$GONE: `add Kind.HUGE downgrade GONE`, in the migration from API version v2, names example.shop#Kind$GONE, which API version v2 does not have; GetItem is not converted across it",
                "example.shop#Kind$GONE: `add Kind.HUGE downgrade GONE`, in the migration from API version v2, names example.shop#Kind$GONE, which API version v2 does not have; ListItems is not converted across it",
                "example.shop#Kind$GONE: `add Kind.HUGE downgrade GONE`, in the migration from API version v2, names example.shop#Kind$GONE, which API version v2 does not have; ListLabels is not converted across it",
            ],
        ),
        (
            "add enum value, the value",
            1,
            Box::new(|step| match step {
                VersionStep::AddEnumValue { value, .. } => {
                    "GONE".clone_into(value);
                    true
                }
                _ => false,
            }),
            &[
                "example.shop#Kind$GONE: `add Kind.GONE downgrade LARGE`, in the migration from API version v2, names example.shop#Kind$GONE, which API version v3 does not have; GetItem is not converted across it",
                "example.shop#Kind$GONE: `add Kind.GONE downgrade LARGE`, in the migration from API version v2, names example.shop#Kind$GONE, which API version v3 does not have; ListItems is not converted across it",
                "example.shop#Kind$GONE: `add Kind.GONE downgrade LARGE`, in the migration from API version v2, names example.shop#Kind$GONE, which API version v3 does not have; ListLabels is not converted across it",
            ],
        ),
        (
            "add error",
            1,
            Box::new(|step| match step {
                VersionStep::AddError { error, .. } => {
                    *error = "example.shop#Gone".parse().unwrap();
                    true
                }
                _ => false,
            }),
            &[
                "example.shop#Gone: `add GetItem.errors Gone downgrade ItemNotFoundException`, in the migration from API version v2, names example.shop#Gone, which the current model does not have; GetItem is not converted across it",
            ],
        ),
        (
            "reshape",
            1,
            Box::new(|step| match step {
                VersionStep::Hooked {
                    hook: Hook::Reshape { old, .. },
                    ..
                } => {
                    *old = "example.shop#Gone".parse().unwrap();
                    true
                }
                _ => false,
            }),
            &[
                "example.shop#Gone: `reshape Gone`, in the migration from API version v2, names example.shop#Gone, which API version v2 does not have; GetItem is not converted across it",
            ],
        ),
        (
            "retype, older side",
            1,
            Box::new(|step| match step {
                VersionStep::Hooked {
                    hook: Hook::Retype { member, .. },
                    ..
                } if member == "force" => {
                    "gone".clone_into(member);
                    true
                }
                _ => false,
            }),
            &[
                "example.shop#DeleteItemInput$gone: `retype DeleteItemInput.gone by hook`, in the migration from API version v2, names example.shop#DeleteItemInput$gone, which API version v2 does not have; DeleteItem is not converted across it",
            ],
        ),
        (
            "retype, newer side",
            1,
            Box::new(|step| match step {
                VersionStep::Hooked {
                    hook: Hook::Retype { member, from, .. },
                    ..
                } if member == "force" => {
                    "force".clone_into(from);
                    "gone".clone_into(member);
                    true
                }
                _ => false,
            }),
            &[
                "example.shop#DeleteItemInput$gone: `retype DeleteItemInput.gone by hook`, in the migration from API version v2, names example.shop#DeleteItemInput$gone, which API version v3 does not have; DeleteItem is not converted across it",
            ],
        ),
        (
            "require by hook",
            0,
            Box::new(|step| match step {
                VersionStep::Hooked {
                    hook: Hook::Require { member, .. },
                    ..
                } => {
                    "gone".clone_into(member);
                    true
                }
                _ => false,
            }),
            &[
                "example.shop#ListItemsInput$gone: `require ListItemsInput.gone by hook`, in the migration from API version v1, names example.shop#ListItemsInput$gone, which API version v2 does not have; ListItems is not converted across it",
            ],
        ),
        // A shape neither release has bears on no operation, and is
        // reported once for the step.
        (
            "a shape neither release has",
            0,
            Box::new(move |step| match step {
                VersionStep::Rename { old, new, from, .. } if from == "name" => {
                    *old = gone.clone();
                    *new = gone.clone();
                    true
                }
                _ => false,
            }),
            &[
                "example.shop#Gone$name: `rename Gone.name -> title`, in the migration from API version v1, names example.shop#Gone$name, which neither API version v1 nor v2 has; no operation is converted across it",
            ],
        ),
    ];
    for (name, index, edit, expected) in cases {
        let mut service = fixture.clone();
        let edited = service.versions[index]
            .steps
            .iter_mut()
            .filter_map(|step| edit(step).then_some(()))
            .count();
        assert_eq!(edited, 1, "{name}: one step edited");
        let reported = reported(&service);
        // Once for each operation whose values reach the step's shape,
        // however many older versions' chains cross it; not for the others.
        let unresolved: Vec<&str> = reported
            .iter()
            .map(String::as_str)
            .filter(|line| line.ends_with("converted across it"))
            .collect();
        assert_eq!(unresolved, expected, "{name}: {reported:#?}");
    }
}

#[test]
fn a_conversion_refused_still_collects_every_problem() {
    use crucible_ir::ir::{Hook, RetypeBy, VersionStep};

    let mut service =
        lower(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned"));
    // DeleteItem's input retype is served by a declared conversion, which
    // is not generated, and its output retype names a member v3 lacks.
    for step in &mut service.versions[1].steps {
        if let VersionStep::Hooked {
            hook: Hook::Retype { member, by, .. },
            ..
        } = step
        {
            if member == "force" {
                *by = RetypeBy::Conversion;
            } else if member == "deleted" {
                "gone".clone_into(member);
            }
        }
    }
    let reported = reported(&service);
    assert!(
        reported
            .iter()
            .any(|line| line.contains("needs a declared `conversion`")),
        "{reported:#?}"
    );
    assert!(
        reported
            .iter()
            .any(|line| line.contains("`retype DeleteItemOutput.gone by hook`")),
        "{reported:#?}"
    );
}

#[test]
fn an_enums_value_changes_map_in_one_lookup() {
    // TINY takes SMALL's place and HUGE takes SMALL's wire value: one map,
    // so a TINY sent down as `small` is not then read as HUGE's `large`.
    let v1 = r#"service Shop {
    version "v1"
    protocol json
    operations [GetItem]
    errors [ValidationException, InternalServerException]
}

readonly operation GetItem {
    http GET "/items/{id}"
    input { id: string }
    output { kind: Kind }
}

enum Kind { SMALL = "small", LARGE = "large" }
"#;
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("versions-enum-composed");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    freeze(&dir, v1);
    fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v1.json\"\n"),
    )
    .unwrap();
    let current = v1.replace("version \"v1\"", "version \"v2\"").replace(
        "enum Kind { SMALL = \"small\", LARGE = \"large\" }",
        "enum Kind { TINY = \"tiny\", LARGE = \"large\", HUGE = \"small\" }",
    );
    fs::write(
        dir.join("shop.cru"),
        format!(
            "{current}\nmigration \"v2\" from \"v1\" {{\n    rename Kind.SMALL -> TINY\n    add Kind.HUGE downgrade LARGE\n}}\n"
        ),
    )
    .unwrap();
    let code = generate(&lower(&dir), &Options::default()).unwrap();
    let expected = r#"downgrade: &[Transform::Map { at: &[Seg::Field("kind")], values: &[("small", "large"), ("tiny", "small")] }]"#;
    assert!(code.contains(expected), "{expected}\n{code}");
}

#[test]
fn a_renamed_enum_value_keying_a_map_in_an_error_is_reported() {
    // The enum is reached only as the key of a map an error holds: not a
    // place a value sits, yet a value that changes on the wire.
    let v1 = r#"service Shop {
    version "v1"
    protocol json
    operations [GetItem]
    errors [ValidationException, InternalServerException]
}

readonly operation GetItem {
    http GET "/items/{id}"
    input { id: string }
    errors [Missing]
    output { name: string }
}

error Missing {
    status 404
    message: string
    counts?: Counts
}

map Counts: Kind -> integer

enum Kind { SMALL, LARGE }
"#;
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("versions-enum-error-key");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    freeze(&dir, v1);
    fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v1.json\"\n"),
    )
    .unwrap();
    let current = v1
        .replace("version \"v1\"", "version \"v2\"")
        .replace("enum Kind { SMALL, LARGE }", "enum Kind { TINY, LARGE }");
    fs::write(
        dir.join("shop.cru"),
        format!("{current}\nmigration \"v2\" from \"v1\" {{\n    rename Kind.SMALL -> TINY\n}}\n"),
    )
    .unwrap();
    let reported = reported(&lower(&dir));
    assert!(
        reported.iter().any(|line| line
            == "example.shop#Kind: a renamed enum value inside error example.shop#Missing is not converted for API version v1 yet"),
        "{reported:#?}"
    );
}

/// A shelf, release v1: one operation answers it, so the migration that
/// retypes its width converts it down only.
const SHELF_V1: &str = r#"service Shop {
    version "v1"
    protocol json
    operations [GetShelf]
    errors [ValidationException, InternalServerException]
}

readonly operation GetShelf {
    http GET "/shelf"
    output { shelf: Shelf }
}

structure Shelf {
    name: string
    width?: integer
}
"#;

/// The shelf at v2, its width retyped by hook, with `additions` appended to
/// the source and `shelf` the members `Shelf` has.
fn shelf_package(name: &str, operations: &str, shelf: &str, additions: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("versions-hooks-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    freeze(&dir, SHELF_V1);
    fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v1.json\"\n"),
    )
    .unwrap();
    let current = SHELF_V1
        .replace("version \"v1\"", "version \"v2\"")
        .replace(
            "operations [GetShelf]",
            &format!("operations [{operations}]"),
        )
        .replace(
            "    name: string\n    width?: integer\n",
            &format!("    name: string\n    width?: string\n{shelf}"),
        );
    fs::write(
        dir.join("shop.cru"),
        format!(
            "{current}{additions}\nmigration \"v2\" from \"v1\" {{\n    retype Shelf.width -> string by hook\n}}\n"
        ),
    )
    .unwrap();
    dir
}

/// Every `Migration` trait in `code`, oldest first, documentation left out:
/// what a service's hand-written hooks implement.
fn hook_traits(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines = code.lines();
    while let Some(line) = lines.next() {
        if !line.trim_start().starts_with("pub trait Migration:") {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let mut decl = vec![line.trim().to_owned()];
        for line in lines.by_ref() {
            if !line.trim_start().starts_with("///") {
                decl.push(line.trim().to_owned());
            }
            if line.len() - line.trim_start().len() == indent && line.trim() == "}" {
                break;
            }
        }
        out.push(decl.join("\n"));
    }
    out
}

/// Decision 82: a hook converts one shape in one direction, whatever
/// operations reach it. An operation added since whose values reach a
/// hooked shape in a direction the migration converts already, and a member
/// appended to that shape, leave the trait as the service implemented it:
/// only the documentation of the method they reuse names the new operation.
#[test]
fn an_addition_reaching_a_hooked_shape_leaves_its_trait_unchanged() {
    let before = generate(
        &lower(&shelf_package("before", "GetShelf", "", "")),
        &Options::default(),
    )
    .unwrap();
    let after = generate(
        &lower(&shelf_package(
            "additive",
            "GetShelf, ListShelves",
            "    depth?: integer\n",
            "\nreadonly operation ListShelves {\n    http GET \"/shelves\"\n    output { shelves: Shelves }\n}\n\nlist Shelves: Shelf\n",
        )),
        &Options::default(),
    )
    .unwrap();
    let traits = hook_traits(&before);
    assert_eq!(
        traits,
        [
            "pub trait Migration: Send + Sync + 'static {\nfn downgrade_shelf_width(&self, value: ::std::string::String) -> ::crucible_server_runtime_rust::__generated::Result<i32>;\n}"
        ]
    );
    assert_eq!(hook_traits(&after), traits);
    assert!(before.contains("/// Needed by `GetShelf`.\n"), "{before}");
    assert!(
        after.contains("/// Needed by `GetShelf` and `ListShelves`.\n"),
        "{after}"
    );
    // The addition reaches v1, through the hook it reuses.
    assert!(
        after.contains(r#"static LIST_SHELVES_0: Conversion = Conversion { upgrade: &[], downgrade: &[Transform::Hook { at: &[Seg::Field("shelves"), Seg::Items, Seg::Field("width")], hook: "versions::v1::Migration::downgrade_shelf_width" }]"#),
        "{after}"
    );
}

/// Decision 75, revisited by 82: an operation added since that sends a
/// hooked shape the other way needs a conversion nobody has written. The
/// trait gains that one method, named for the shape and direction and
/// documented with the operation that needs it, and keeps every other.
#[test]
fn an_addition_needing_a_new_direction_adds_that_method_alone() {
    let code = generate(
        &lower(&shelf_package(
            "direction",
            "GetShelf, PutShelf",
            "",
            "\nidempotent operation PutShelf {\n    http PUT \"/shelf\"\n    input { shelf: Shelf }\n}\n",
        )),
        &Options::default(),
    )
    .unwrap();
    assert_eq!(
        hook_traits(&code),
        [
            "pub trait Migration: Send + Sync + 'static {\nfn downgrade_shelf_width(&self, value: ::std::string::String) -> ::crucible_server_runtime_rust::__generated::Result<i32>;\n\nfn upgrade_shelf_width(&self, value: i32) -> ::crucible_server_runtime_rust::__generated::Result<::std::string::String>;\n}"
        ]
    );
    assert!(
        code.contains("/// `retype Shelf.width`: an older request's value, as the next version reads it.\n                ///\n                /// Needed by `PutShelf`.\n"),
        "{code}"
    );
}

/// A strict structure only the release's inputs reached stays strict at
/// that release, though an operation added since sends it in an output: the
/// release's output reachability decides, not the projection's. One the
/// release's own outputs reached keeps reading what it does not define.
#[test]
fn an_operation_added_since_leaves_an_input_only_strict_structure_strict() {
    let v1 = r#"service Notes {
    version "v1"
    protocol json
    operations [PutNote, GetDraft]
    errors [ValidationException, InternalServerException]
}

idempotent operation PutNote {
    http PUT "/notes/{id}"
    input { id: string, note: Note }
    output { ok: boolean }
}

strict structure Note {
    text: string
}

idempotent operation PutDraft {
    http PUT "/drafts/{id}"
    input { id: string, draft: Draft }
    output { draft: Draft }
}

readonly operation GetDraft {
    http GET "/drafts/{id}"
    input { id: string }
    output { draft: Draft }
}

strict structure Draft {
    text: string
}
"#
    .replace(
        "operations [PutNote, GetDraft]",
        "operations [PutNote, PutDraft, GetDraft]",
    );
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("versions-strict-release");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    freeze(&dir, &v1);
    fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v1.json\"\n"),
    )
    .unwrap();
    let current = v1
        .replace("version \"v1\"", "version \"v2\"")
        .replace("PutDraft, GetDraft]", "PutDraft, GetDraft, GetNote]")
        + "\nreadonly operation GetNote {\n    http GET \"/notes/{id}\"\n    input { id: string }\n    output { note: Note }\n}\n\nmigration \"v2\" from \"v1\" {}\n";
    fs::write(dir.join("shop.cru"), current).unwrap();
    let code = generate(&lower(&dir), &Options::default()).unwrap();
    let v1 = &code[code.find("pub mod v1 {").expect("a v1 module")..];
    let strict = |name: &str| {
        let at = v1
            .find(&format!("pub struct {name} {{"))
            .unwrap_or_else(|| panic!("v1 has {name}"));
        v1[..at]
            .trim_end()
            .ends_with("#[serde(deny_unknown_fields)]")
    };
    // GetNote, added since, sends Note at v1 too; v1 decoded it strictly.
    assert!(strict("Note"), "{v1}");
    // GetDraft's output reached Draft in v1 already.
    assert!(!strict("Draft"), "{v1}");
    // Frozen output reachability makes the old Draft lenient while the
    // current Draft is strict, despite otherwise identical IR shapes.
    assert!(code.contains(".add_versioned::<v1::PutDraft, super::PutDraft>(V1,"));
    assert!(!code.contains(".add_versioned_identity::<v1::PutDraft, super::PutDraft>"));
    // Note was never lenient in the frozen release, so its empty chain can
    // safely decode straight into the current strict type.
    assert!(code.contains(".add_versioned_identity::<v1::PutNote, super::PutNote>(V1,"));
    let strict_inputs = generate(
        &lower(&dir),
        &Options {
            strict_inputs: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(strict_inputs.contains(".add_versioned::<v1::PutDraft, super::PutDraft>(V1,"));
    assert!(strict_inputs.contains(".add_versioned_identity::<v1::PutNote, super::PutNote>(V1,"));
}

/// An operation the current model has is in every projection; one a
/// projection lost, which only an edited IR or a projection bug makes, is
/// reported rather than left unserved at that release.
#[test]
fn an_operation_a_projection_lost_is_reported() {
    let mut service = lower(&model("version-retired-operation"));
    service.versions[0]
        .service
        .operations
        .retain(|op| op.name() != "GetItem");
    assert_eq!(
        reported(&service),
        [
            "example.gone#GetItem: GetItem is in the current model but not in API version v1 as it sees it; nothing serves it at v1"
        ]
    );
}

/// An SDK line (decision 76) is one release as the current model sees it;
/// a server serves every release from the current model, so the generator
/// refuses one, and generates the current release as always.
#[test]
fn a_server_is_never_generated_from_an_sdk_line() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    let current = lower(&fixture);
    let line = crucible_ir::versions::line(&current, "v2").unwrap();
    let error = generate(&line, &Options::default())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("this model is release \"v2\" as the current model sees it, an SDK line"),
        "{error}"
    );
    let same = crucible_ir::versions::line(&current, "v4").unwrap();
    assert!(generate(&same, &Options::default()).is_ok());
}

/// The client of release v1, an SDK line: v1's types and routes, the
/// operation v3 removed, the operations added since, and v1 as the release
/// every operation names.
#[test]
fn the_versioned_fixtures_v1_client_matches_golden() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    let line = crucible_ir::versions::line(&lower(&fixture), "v1").unwrap();
    let client = crucible_ir::client_view(
        &crucible_ir::project(&line, crucible_ir::Audience::External).unwrap(),
    );
    let code = crucible_codegen_rust::generate_client(&client).unwrap();
    assert!(code.contains(r#"::std::option::Option::Some("v1")"#));
    assert!(!code.contains(r#"::std::option::Option::Some("v4")"#));
    assert!(code.contains("CountItems"));
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/versioned_v1_client.rs");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::write(&path, &code).unwrap();
    }
    let golden = fs::read_to_string(&path).expect("golden exists; bless to create it");
    assert!(
        golden.replace("\r\n", "\n") == code,
        "generated code differs from {}; rerun with CRUCIBLE_BLESS=1 and read the diff",
        path.display()
    );
}

#[test]
fn an_identical_empty_chain_registers_current_codecs_at_the_older_version() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    let code = generate(&lower(&fixture), &Options::default()).unwrap();
    for (module, version) in [("v1", "V1"), ("v2", "V2"), ("v3", "V3")] {
        assert!(code.contains(&format!(
            ".add_versioned_identity::<{module}::RecordPayment, super::RecordPayment>({version},"
        )), "{module}: unchanged payment contract should use the identity path");
    }
    // A real conversion retains its typed migration path, even though
    // another operation in the same release is an identity.
    assert!(code.contains(".add_versioned::<v1::GetItem, super::GetItem>(V1,"));
    assert!(code.contains(".add_versioned::<v1::CountItems, v2::CountItems>(V1,"));
}

#[test]
fn changed_types_errors_and_bindings_keep_the_conversion_path() {
    use crucible_ir::ShapeId;
    use crucible_ir::ir::Shape;

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    let current = lower(&fixture);
    for changed in ["input", "output", "nested", "error", "binding"] {
        let mut service = current.clone();
        let older = &mut service.versions[0].service;
        let operation = older
            .operations
            .iter_mut()
            .find(|op| op.name() == "RecordPayment")
            .unwrap();
        match changed {
            "binding" => operation.request.headers[0].name = "Old-Payment-Signature".into(),
            "input" | "output" => {
                let id = if changed == "input" {
                    operation.input.clone()
                } else {
                    operation.output.clone()
                }
                .unwrap();
                let Shape::Structure(shape) = older.shapes.get_mut(&id).unwrap() else {
                    panic!("body");
                };
                shape.members[0].required = false;
            }
            "nested" => {
                let input = operation.input.as_ref().unwrap();
                let id = older.shapes[input].members()[0].target.clone();
                let Shape::Simple(shape) = older.shapes.get_mut(&id).unwrap() else {
                    panic!("nested string");
                };
                shape.sensitive = !shape.sensitive;
            }
            "error" => {
                let id = ShapeId::new("crucible.errors", "ValidationException");
                let Shape::Structure(shape) = older.shapes.get_mut(&id).unwrap() else {
                    panic!("error");
                };
                shape.error.as_mut().unwrap().status = 422;
            }
            _ => unreachable!(),
        }
        let code = generate(&service, &Options::default()).unwrap();
        assert!(
            code.contains(".add_versioned::<v1::RecordPayment, super::RecordPayment>(V1,"),
            "{changed}"
        );
        assert!(
            !code.contains(".add_versioned_identity::<v1::RecordPayment, super::RecordPayment>"),
            "{changed}"
        );
    }
}

/// Equal IR closures alone do not prove a Rust value survives the removed
/// typed/JSON round trip. Keep unproven scalar representations conservative.
#[test]
fn unproven_scalar_round_trips_keep_the_existing_conversion_path() {
    use crucible_ir::ShapeId;
    use crucible_ir::ir::{Shape, SimpleKind};

    fn scalar_output(model: &mut Service, kind: SimpleKind) {
        let output = model
            .operations
            .iter()
            .find(|op| op.name() == "RecordPayment")
            .unwrap()
            .output
            .clone()
            .unwrap();
        let Shape::Structure(body) = &model.shapes[&output] else {
            panic!("output");
        };
        let mut scalar = model.shapes[&body.members[0].target].clone();
        let Shape::Simple(value) = &mut scalar else {
            panic!("scalar");
        };
        value.kind = kind;
        let target = ShapeId::new("example.shop", "Measured");
        model.shapes.insert(target.clone(), scalar);
        let Shape::Structure(body) = model.shapes.get_mut(&output).unwrap() else {
            panic!("output");
        };
        body.members[0].target = target;
    }

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    let current = lower(&fixture);
    for kind in [
        SimpleKind::Float,
        SimpleKind::Double,
        SimpleKind::Timestamp,
        SimpleKind::BigInteger,
        SimpleKind::BigDecimal,
    ] {
        let mut service = current.clone();
        scalar_output(&mut service, kind);
        for version in &mut service.versions {
            scalar_output(&mut version.service, kind);
        }
        let code = generate(&service, &Options::default()).unwrap();
        assert!(
            code.contains(".add_versioned::<v1::RecordPayment, super::RecordPayment>(V1,"),
            "{kind:?}"
        );
        assert!(
            !code.contains(".add_versioned_identity::<v1::RecordPayment, super::RecordPayment>"),
            "{kind:?}"
        );
    }
}
