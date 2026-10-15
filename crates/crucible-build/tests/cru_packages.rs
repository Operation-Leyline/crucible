#![cfg(feature = "rust")]
//! A directory whose `crucible.toml` has a `[model]` table is a Crucible
//! language package: `model(..)` compiles it, and
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

#[test]
fn cru_package_export_probe() {
    if std::env::var_os("CRUCIBLE_CRU_EXPORT_PROBE").is_none() {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    crucible_build::model("api")
        .server()
        .model_output(out.join("export/model.json"))
        .generate();
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
fn a_cru_package_compiles_publishes_and_generates() {
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
        "no intermediate compilation output is written"
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

#[test]
fn a_cru_package_exports_its_compiled_model_with_metadata_and_docs() {
    let source = WEATHER.replace(
        "operation GetCity reads City {",
        "/// Fetches one city.
@note(\"kept\")
operation GetCity reads City {",
    ) + "
/// Marks an operation.
metadata note on operation: string
";
    let root = package(&source);
    let output = run(
        root.path(),
        "cru_package_export_probe",
        "CRUCIBLE_CRU_EXPORT_PROBE",
    );
    assert!(
        output.status.success(),
        "{}
{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let exported = root.path().join("out/export/model.json");
    let text = fs::read_to_string(&exported).unwrap();
    assert!(text.contains("example.weather#note"), "{text}");
    assert!(text.contains("\"kept\""), "{text}");
    assert!(text.contains("Fetches one city."), "{text}");
    // The same file a freeze would write: it reads back as a release.
    crucible_smithy::snapshot::read(&text).expect("a snapshot");

    // Unchanged contents leave the file alone, so a watched export does not
    // rerun the build that wrote it.
    let before = fs::metadata(&exported).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    let again = run(
        root.path(),
        "cru_package_export_probe",
        "CRUCIBLE_CRU_EXPORT_PROBE",
    );
    assert!(again.status.success());
    assert_eq!(before, fs::metadata(&exported).unwrap().modified().unwrap());
}
