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
fn check_without_a_target_accepts_a_sparse_collection() {
    // A gated feature every generator supports must not fail the plain check:
    // the empty capability set used to refuse `sparse` as unsupported.
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let dir = tmp.path();
    weather_copy(dir);
    let model = dir.join("model/weather.cru");
    // Referenced from a reachable structure: an unreferenced shape records
    // no feature use, so it would not exercise the capability check.
    let text = std::fs::read_to_string(&model).unwrap().replace(
        "structure Coordinates {
",
        "structure Coordinates {
    readings?: Readings
",
    );
    assert!(text.contains("readings?: Readings"));
    let text = text
        + "
/// Readings that may be missing.
sparse list Readings: string { length ..10 }
";
    std::fs::write(&model, text).unwrap();
    let check = crucible(&["check", dir.to_str().unwrap()]);
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
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
    let frozen = dir.join("releases/2026-10-01.json");

    // Freeze the release: a snapshot of its compiled model, recorded in the lock.
    let freeze = crucible(&["freeze", package]);
    assert!(
        freeze.status.success(),
        "{}",
        String::from_utf8_lossy(&freeze.stderr)
    );
    assert!(frozen.is_file());
    let lock = std::fs::read_to_string(dir.join("releases/crucible.lock")).unwrap();
    assert!(lock.contains("\"2026-10-01.json\" = \"sha256:"), "{lock}");
    // A frozen release is never overwritten.
    assert!(!crucible(&["freeze", package]).status.success());

    let manifest = std::fs::read_to_string(dir.join("crucible.toml")).unwrap();
    std::fs::write(
        dir.join("crucible.toml"),
        manifest.replace(
            "[model.naming]",
            "[model.baseline]\npath = \"releases/2026-10-01.json\"\n\n[model.naming]",
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
fn a_past_release_is_frozen_from_its_git_commit() {
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let repo = tmp.path();
    // The package's path repeats inside it (`model/model/weather.cru`), and
    // its manifest names a baseline in the old layout, missing at that commit.
    let dir = repo.join("model");
    std::fs::create_dir(&dir).unwrap();
    weather_copy(&dir);
    let manifest = std::fs::read_to_string(dir.join("crucible.toml")).unwrap();
    std::fs::write(
        dir.join("crucible.toml"),
        manifest.replace(
            "[model.naming]",
            "[model.baseline]\npath = \"releases/old\"\n\n[model.naming]",
        ),
    )
    .unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args([
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.invalid",
            ])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "release 2026-10-01"]);
    git(&["tag", "release-1"]);
    // The model moves on without the release ever being frozen.
    let model = dir.join("model/weather.cru");
    let source = std::fs::read_to_string(&model).unwrap();
    std::fs::write(
        &model,
        source.replace("version \"2026-10-01\"", "version \"2026-11-01\""),
    )
    .unwrap();
    git(&["commit", "-qam", "next"]);

    let freeze = crucible(&["freeze", dir.to_str().unwrap(), "--from", "release-1"]);
    assert!(
        freeze.status.success(),
        "{}",
        String::from_utf8_lossy(&freeze.stderr)
    );
    let snapshot = std::fs::read_to_string(dir.join("releases/2026-10-01.json")).unwrap();
    assert!(snapshot.contains("\"version\": \"2026-10-01\""));
    assert!(dir.join("releases/crucible.lock").is_file());
    // An unknown commit is refused.
    assert!(
        !crucible(&["freeze", dir.to_str().unwrap(), "--from", "no-such-tag"])
            .status
            .success()
    );
}

#[test]
fn compile_json_prints_the_model_with_custom_metadata_and_docs() {
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let dir = tmp.path();
    std::fs::write(
        dir.join("crucible.toml"),
        "[model]\nnamespace = \"example.meta\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("a.cru"),
        r#"service Meta { version "1"  protocol json  operations [Ping] }

/// Marks an operation.
metadata note on operation: string

/// Ping the service.
@note("kept")
readonly operation Ping {
    http GET "/ping"
    output { ok: boolean }
}
"#,
    )
    .unwrap();
    let path = dir.to_str().unwrap();

    let printed = crucible(&["compile", "--json", path]);
    assert!(
        printed.status.success(),
        "{}",
        String::from_utf8_lossy(&printed.stderr)
    );
    let text = String::from_utf8(printed.stdout).unwrap();
    assert!(text.contains("example.meta#note"), "{text}");
    assert!(
        text.contains("kept") && text.contains("Ping the service."),
        "{text}"
    );
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["service"]["operations"][0]["id"], "example.meta#Ping");

    // --out writes the same bytes to a file.
    let file = dir.join("not-yet/model.json");
    let written = crucible(&["compile", "--json", path, "--out", file.to_str().unwrap()]);
    assert!(written.status.success());
    assert!(written.stdout.is_empty());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);

    // Without --json there is no output format yet, and a non-package is refused.
    assert!(!crucible(&["compile", path]).status.success());
    let refused = crucible(&["compile", "--json", env!("CARGO_MANIFEST_DIR")]);
    assert!(String::from_utf8_lossy(&refused.stderr).contains("not a Crucible language package"));
}
