//! Shapes may be named like identifiers the generated code uses: `Option`,
//! `Fault`, `ErrorInfo`, `Validator`, `Validate`. The generated server module
//! and SDK must still compile, so each is checked with `cargo check`.
#[path = "../../../tests/support/cargo_target.rs"]
mod cargo_target;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crucible_codegen_rust::{
    ClientOptions, Options, RuntimeDependency, emit_client_package, generate,
};
use crucible_smithy::Model;
use serde_json::{Value, json};

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

fn service() -> crucible_smithy::ir::Service {
    let mut ast: Value = serde_json::from_str(WEATHER).unwrap();
    ast["shapes"]["example.weather#Weather"]["rename"] = json!({
        "example.weather#Coordinates": "Option",
        "example.weather#ValidationField": "Validator",
        "example.weather#Location": "Validate",
        "example.weather#NotFoundError": "Fault",
        "example.weather#ConflictError": "ErrorInfo",
    });
    // An intEnum generates `from_i32 -> Option<Self>`.
    ast["shapes"]["example.weather#Level"] = json!({ "type": "intEnum", "members": {
        "LOW": { "target": "smithy.api#Unit", "traits": { "smithy.api#enumValue": 1 } },
        "HIGH": { "target": "smithy.api#Unit", "traits": { "smithy.api#enumValue": 2 } },
    } });
    ast["shapes"]["example.weather#CitySummary"]["members"]["level"] =
        json!({ "target": "example.weather#Level" });
    Model::from_json(&ast.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_rust::capabilities())
        .unwrap()
}

fn check(manifest: &Path) {
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["check", "--manifest-path"])
        .arg(manifest)
        .env("CARGO_TARGET_DIR", cargo_target::target_dir())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn crate_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(name)
        .canonicalize()
        .unwrap()
}

#[test]
fn server_code_compiles_with_shapes_named_like_bare_identifiers() {
    let directory = tempfile::tempdir().unwrap();
    let source = generate(&service(), &Options::default()).unwrap();
    fs::create_dir_all(directory.path().join("src")).unwrap();
    fs::write(directory.path().join("src/lib.rs"), source).unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        format!(
            "[package]\nname = 'bare-identifiers-server'\nversion = '0.0.0'\nedition = '2024'\n[dependencies]\ncrucible-server-runtime-rust = {{ path = {:?} }}\n[workspace]\n",
            crate_path("crucible-server-runtime-rust")
        ),
    )
    .unwrap();
    check(&directory.path().join("Cargo.toml"));
}

#[test]
fn sdk_compiles_with_shapes_named_like_bare_identifiers() {
    let directory = tempfile::tempdir().unwrap();
    let files = emit_client_package(
        &service(),
        &ClientOptions {
            package_name: Some("bare-identifiers-sdk".into()),
            package_version: "0.0.0".into(),
            runtime: RuntimeDependency::Path(crate_path("crucible-client-runtime-rust")),
            metadata: Default::default(),
        },
    )
    .unwrap();
    for file in files {
        let path = directory.path().join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
    check(&directory.path().join("Cargo.toml"));
}
