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

    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cru-package-ts");
    let _ = std::fs::remove_dir_all(&out);
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
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cru-package-broken");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
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
