#![cfg(feature = "rust")]

mod common;

use std::path::{Path, PathBuf};

use crucible_build::model;

fn fixture() -> PathBuf {
    common::shared_package("policy-contract")
}

#[test]
fn explicit_policy_is_required_and_applied_before_generation() {
    let root = std::env::temp_dir().join(format!("crucible-build-policy-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("crucible.toml");
    let codegen = model(fixture()).server().custom_rejections().policy(&path);
    assert!(codegen.render().unwrap_err().contains("cannot read"));
    std::fs::write(&path, "schema_version = 1\n[rules.paths]\nprefix = '/v2'").unwrap();
    assert!(codegen.render().unwrap_err().contains("paths.prefix"));
    std::fs::write(&path, "schema_version = 1\n[rules.paths]\nprefix = '/v1'").unwrap();
    assert!(codegen.render().is_ok());
    std::fs::write(
        &path,
        "schema_version = 1\n[rules.paths]\nprefix = '/v2'\nseverity = 'warning'",
    )
    .unwrap();
    assert!(codegen.render().is_ok());
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&root).unwrap();
}

/// Copies the weather example package, with `policy` appended to its manifest.
fn weather_package(name: &str, policy: &str) -> PathBuf {
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/idl/examples/weather");
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("model")).unwrap();
    for entry in std::fs::read_dir(example.join("model")).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, root.join("model").join(path.file_name().unwrap())).unwrap();
    }
    let manifest = std::fs::read_to_string(example.join("crucible.toml")).unwrap();
    std::fs::write(root.join("crucible.toml"), format!("{manifest}\n{policy}")).unwrap();
    root
}

#[test]
fn a_package_manifest_policy_applies_without_selecting_it() {
    let package = weather_package(
        "policy-in-manifest-error",
        "[rules.naming]\noperations = \"camelCase\"\n",
    );
    let error = model(&package).server().render().unwrap_err();
    assert!(error.contains("naming.operations"), "{error}");
    assert!(error.contains("is not camelCase"), "{error}");

    let package = weather_package(
        "policy-in-manifest-warning",
        "[rules.naming]\noperations = { case = \"camelCase\", severity = \"warning\" }\n",
    );
    assert!(model(&package).server().render().is_ok());
}

/// The weather package with a policy warning and a compiler warning: a
/// naming rule at warning severity, and a doc comment that documents
/// nothing.
fn warning_package(name: &str) -> PathBuf {
    let package = weather_package(
        name,
        "[rules.naming]\noperations = { case = \"camelCase\", severity = \"warning\" }\n",
    );
    let model = package.join("model/weather.cru");
    let text = std::fs::read_to_string(&model).unwrap();
    std::fs::write(&model, format!("{text}\n/// Documents nothing.\n")).unwrap();
    package
}

/// The `cargo::warning` lines a build of `package` prints, run in a child
/// test: Cargo reads them from stdout, which the test harness captures.
/// The build lowers the model three times: server bindings, the model
/// export beside them, and the compiled package `publish()` checks.
fn printed_warnings(package: &Path, report: bool) -> Vec<String> {
    let out = package.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["print_the_model_warnings", "--exact", "--nocapture"])
        .env("CRUCIBLE_BUILD_TEST_PACKAGE", package)
        .env("CRUCIBLE_BUILD_TEST_REPORT", report.to_string())
        .env("OUT_DIR", &out)
        .env_remove("CRUCIBLE_MODEL_OUT")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| line.starts_with("cargo::warning="))
        .map(str::to_owned)
        .collect()
}

/// One build that lowers its model several times prints each of the
/// model's warnings once.
#[test]
fn a_build_prints_each_model_warning_once() {
    let package = warning_package("policy-reported-once");
    let printed = printed_warnings(&package, true);
    for expected in ["naming.operations", "documents nothing"] {
        assert!(
            printed.iter().any(|line| line.contains(expected)),
            "{expected}: {printed:?}"
        );
    }
    for line in &printed {
        let times = printed.iter().filter(|other| *other == line).count();
        assert_eq!(times, 1, "{line} printed {times} times: {printed:?}");
    }
}

/// Two builds that compile one model (a model crate that publishes it, and
/// a service that generates from it) print its findings once: the second
/// leaves them, compiler warnings included, to the first with
/// `report_warnings(false)`, and still fails on an error finding.
#[test]
fn a_build_can_leave_the_model_warnings_to_another() {
    let package = warning_package("policy-reported-elsewhere");
    assert_eq!(printed_warnings(&package, false), Vec::<String>::new());

    let package = weather_package(
        "policy-reported-elsewhere-error",
        "[rules.naming]\noperations = \"camelCase\"\n",
    );
    let error = model(&package)
        .report_warnings(false)
        .server()
        .render()
        .unwrap_err();
    assert!(error.contains("naming.operations"), "{error}");
}

/// The child of [`printed_warnings`]: does nothing unless it names a
/// package.
#[test]
fn print_the_model_warnings() {
    let Some(package) = std::env::var_os("CRUCIBLE_BUILD_TEST_PACKAGE") else {
        return;
    };
    let package = PathBuf::from(package);
    let report = std::env::var("CRUCIBLE_BUILD_TEST_REPORT").unwrap() == "true";
    let model = model(&package).report_warnings(report);
    model
        .server()
        .model_output(package.join("out/model.json"))
        .generate();
    model.publish();
}
