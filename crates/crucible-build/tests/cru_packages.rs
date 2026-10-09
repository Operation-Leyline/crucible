#![cfg(feature = "rust")]
//! A directory whose `crucible.toml` has a `[model]` table is a Crucible
//! language package: `model(..)` compiles it with no Smithy compiler, and
//! `publish()` hands the directory to dependents.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const WEATHER: &str = include_str!("../../../docs/idl/examples/weather/model/weather.cru");

#[test]
fn cru_package_probe() {
    if std::env::var_os("CRUCIBLE_CRU_PACKAGE_PROBE").is_none() {
        return;
    }
    let model = crucible_build::model("api").compile();
    model.publish();
    let code = model.server().render().unwrap();
    assert!(code.contains("pub mod weather"), "{code}");
    assert!(code.contains("GetCityOperation"));
}

#[test]
fn broken_cru_package_probe() {
    if std::env::var_os("CRUCIBLE_BROKEN_CRU_PROBE").is_none() {
        return;
    }
    let _ = crucible_build::model("api").compile();
}

fn run(root: &Path, probe: &str, variable: &str) -> Output {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", probe, "--nocapture"])
        .current_dir(root)
        .env(variable, "1")
        .env("CARGO_MANIFEST_DIR", root)
        .env("OUT_DIR", root.join("out"))
        .output()
        .unwrap()
}

fn package(source: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("out")).unwrap();
    fs::create_dir_all(root.path().join("api")).unwrap();
    fs::write(
        root.path().join("api/crucible.toml"),
        "[model]\nnamespace = \"example.weather\"\n",
    )
    .unwrap();
    fs::write(root.path().join("api/weather.cru"), source).unwrap();
    root
}

#[test]
fn a_cru_package_compiles_publishes_and_generates_without_smithy() {
    let root = package(WEATHER);
    let output = run(
        root.path(),
        "cru_package_probe",
        "CRUCIBLE_CRU_PACKAGE_PROBE",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let api = root.path().join("api");
    assert!(
        stdout.contains(&format!("cargo::metadata=model={}", api.display())),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("cargo::rerun-if-changed={}", api.display())),
        "{stdout}"
    );
    assert!(
        !root.path().join("out/smithy").exists(),
        "no Smithy compilation ran"
    );
}

#[test]
fn a_broken_cru_package_fails_the_build_with_every_diagnostic() {
    let root = package(
        "service S { version \"1\"\n protocol json }\nstructure A { a: Missing }\nstructure B { b: AlsoMissing }\n",
    );
    let output = run(
        root.path(),
        "broken_cru_package_probe",
        "CRUCIBLE_BROKEN_CRU_PROBE",
    );
    assert!(!output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("does not compile"), "{text}");
    assert!(text.contains("unknown type `Missing`"), "{text}");
    assert!(text.contains("unknown type `AlsoMissing`"), "{text}");
}
