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
