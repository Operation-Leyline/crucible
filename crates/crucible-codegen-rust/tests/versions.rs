//! Older API versions (docs/api-versions.md): the generated `versions`
//! module for the e2e fixture, checked in, and what the generator reports
//! instead of converting. `CRUCIBLE_BLESS=1 cargo test` rewrites the golden.

use std::fs;
use std::path::{Path, PathBuf};

use crucible_codegen_rust::{Options, generate};
use crucible_smithy::ir::Service;

/// The package at `dir`, lowered with the versions it serves.
fn lower(dir: &Path) -> Service {
    let package = crucible_lang::load_package(dir).unwrap();
    let own = package.files.len();
    let compiled = crucible_lang::compile(&package, None);
    let versions = crucible_lang::versions(&package, &compiled, own).unwrap();
    let mut service = compiled.service.expect("the package compiles");
    service.versions = versions;
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

/// A package whose release v1 is `V1`, at v2 with `current` and `migration`.
fn package(name: &str, current: &str, migration: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("versions-{name}"));
    let _ = fs::remove_dir_all(&dir);
    let frozen = dir.join("releases/v1");
    fs::create_dir_all(&frozen).unwrap();
    fs::write(frozen.join("crucible.toml"), MANIFEST).unwrap();
    fs::write(frozen.join("shop.cru"), V1).unwrap();
    fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v1\"\n"),
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
