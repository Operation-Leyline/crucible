//! A Crucible language package is a model source like Smithy sources: the
//! CLI compiles it with `crucible-lang`, with no JVM.

use std::path::Path;
use std::process::Command;

fn crucible(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_crucible"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn check_and_generate_accept_a_package_directory() {
    let weather = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/idl/examples/weather");
    let weather = weather.to_str().unwrap();

    let check = crucible(&["check", weather]);
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(!check.stdout.is_empty());

    // A directory of its own, so concurrent runs never share output.
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let out = tmp.path().join("ts");
    let generate = crucible(&[
        "generate",
        "ts",
        weather,
        "--out",
        out.to_str().unwrap(),
        "--package-name",
        "weather-client",
    ]);
    assert!(
        generate.status.success(),
        "{}",
        String::from_utf8_lossy(&generate.stderr)
    );
    assert!(std::fs::read_dir(&out).unwrap().next().is_some());
}

#[test]
fn a_package_that_does_not_compile_lists_its_problems() {
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let dir = tmp.path();
    std::fs::write(
        dir.join("crucible.toml"),
        "[model]\nnamespace = \"example.broken\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("a.cru"), "structure A { b: Missing }\n").unwrap();
    let check = crucible(&["check", dir.to_str().unwrap()]);
    assert!(!check.status.success());
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        stderr.contains("does not compile") && stderr.contains("Missing"),
        "{stderr}"
    );

    let protocol = crucible(&[
        "check",
        dir.to_str().unwrap(),
        "--protocol",
        "aws.protocols#restJson1",
    ]);
    assert!(String::from_utf8_lossy(&protocol.stderr).contains("--protocol cannot select"));
}

#[test]
fn check_applies_the_policy_in_the_package_manifest() {
    let weather = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/idl/examples/weather");
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let dir = tmp.path();
    std::fs::create_dir(dir.join("model")).unwrap();
    std::fs::copy(
        weather.join("model/weather.cru"),
        dir.join("model/weather.cru"),
    )
    .unwrap();
    let manifest = std::fs::read_to_string(weather.join("crucible.toml")).unwrap();
    std::fs::write(
        dir.join("crucible.toml"),
        format!(
            "{manifest}
[rules.naming]
members = \"PascalCase\"
"
        ),
    )
    .unwrap();
    let check = crucible(&["check", dir.to_str().unwrap()]);
    assert!(!check.status.success());
    let out = String::from_utf8_lossy(&check.stdout).into_owned()
        + &String::from_utf8_lossy(&check.stderr);
    assert!(
        out.contains("naming.members") && out.contains("is not PascalCase"),
        "{out}"
    );
}

/// Copies the weather example into a fresh directory.
fn weather_copy(dir: &Path) {
    let weather = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/idl/examples/weather");
    std::fs::create_dir(dir.join("model")).unwrap();
    std::fs::copy(
        weather.join("model/weather.cru"),
        dir.join("model/weather.cru"),
    )
    .unwrap();
    std::fs::copy(weather.join("crucible.toml"), dir.join("crucible.toml")).unwrap();
}

#[test]
fn a_frozen_baseline_guards_against_breaking_changes() {
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let dir = tmp.path();
    weather_copy(dir);
    let package = dir.to_str().unwrap();
    // The weather example's service is version "2026-10-01", the release's name.
    let frozen = dir.join("releases/2026-10-01");

    // Freeze the release inside the package, under releases/<version> by
    // default; its files are not the package's own (a nested package is
    // its own).
    let freeze = crucible(&["freeze", package]);
    assert!(
        freeze.status.success(),
        "{}",
        String::from_utf8_lossy(&freeze.stderr)
    );
    assert!(frozen.join("model/weather.cru").is_file());
    assert!(
        !std::fs::read_to_string(frozen.join("crucible.toml"))
            .unwrap()
            .contains("baseline")
    );
    // A frozen release is never overwritten.
    assert!(
        !crucible(&["freeze", package, "--out", frozen.to_str().unwrap()])
            .status
            .success()
    );

    let manifest = std::fs::read_to_string(dir.join("crucible.toml")).unwrap();
    std::fs::write(
        dir.join("crucible.toml"),
        manifest.replace(
            "[model.naming]",
            "[model.baseline]\npath = \"releases/2026-10-01\"\n\n[model.naming]",
        ),
    )
    .unwrap();
    let check = crucible(&["check", package]);
    assert!(
        check.status.success(),
        "an unchanged model is compatible: {}",
        String::from_utf8_lossy(&check.stderr)
    );

    // Moving a route breaks old clients.
    let model = dir.join("model/weather.cru");
    let source = std::fs::read_to_string(&model).unwrap();
    std::fs::write(
        &model,
        source.replace("http GET \"/status\"", "http GET \"/health\""),
    )
    .unwrap();
    let check = crucible(&["check", package]);
    assert!(!check.status.success());
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        stderr.contains("breaks compatibility") && stderr.contains("compatibility.http"),
        "{stderr}"
    );
}

#[test]
fn freezing_drops_the_baseline_however_it_is_written() {
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let dir = tmp.path().join("package");
    std::fs::create_dir(&dir).unwrap();
    weather_copy(&dir);
    std::fs::create_dir_all(dir.join("old")).unwrap();
    std::fs::write(
        dir.join("old/crucible.toml"),
        "[model]\nnamespace = \"example.old\"\n",
    )
    .unwrap();
    let manifest = std::fs::read_to_string(dir.join("crucible.toml")).unwrap();
    // A dotted key under [model], not a [model.baseline] table.
    std::fs::write(
        dir.join("crucible.toml"),
        manifest.replace("language = 1", "language = 1\nbaseline.path = \"old\""),
    )
    .unwrap();
    let out = tmp.path().join("frozen");
    let freeze = crucible(&[
        "freeze",
        dir.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(
        freeze.status.success(),
        "{}",
        String::from_utf8_lossy(&freeze.stderr)
    );
    let frozen = std::fs::read_to_string(out.join("crucible.toml")).unwrap();
    assert!(
        !frozen.contains("baseline") && frozen.contains("example.weather"),
        "{frozen}"
    );
}
