#![cfg(all(feature = "rust", feature = "smithy"))]

use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

#[test]
fn package_probe() {
    let Ok(mode) = std::env::var("CRUCIBLE_SDK_PROBE") else {
        return;
    };
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    if mode == "audience" {
        let package = crucible_build::model_from_json("model.json").compile();
        let external = package.rust_client("public").generate();
        let internal = package
            .rust_client("internal")
            .audience(crucible_build::Audience::Internal)
            .generate();
        let contents = |artifact: crucible_build::Artifact| {
            fs::read_to_string(artifact.path.join("src/lib.rs")).unwrap()
        };
        assert!(!contents(external).contains("SecretInput"));
        assert!(contents(internal).contains("SecretInput"));
        let generic = package
            .sdk(
                "generic",
                crucible_build::clients::Client::Rust(Default::default()),
            )
            .generate();
        assert!(!contents(generic).contains("SecretInput"));
        #[cfg(feature = "ts")]
        for (name, audience, visible) in [
            ("ts", crucible_build::Audience::External, false),
            ("ts-internal", crucible_build::Audience::Internal, true),
        ] {
            let artifact = package.ts_client(name).audience(audience).generate();
            let source = fs::read_to_string(artifact.path.join("src/client.ts")).unwrap();
            assert_eq!(source.contains("Secret"), visible);
        }
        #[cfg(feature = "openapi")]
        for (name, audience, visible) in [
            ("api", crucible_build::Audience::External, false),
            ("api-internal", crucible_build::Audience::Internal, true),
        ] {
            let artifact = package.openapi(name).audience(audience).generate();
            let source = fs::read_to_string(artifact.path.join("openapi.json")).unwrap();
            assert_eq!(source.contains("/secret"), visible);
        }
        return;
    }
    if mode == "resolve" {
        let artifact = crucible_build::artifact("source_model", "rust");
        assert_eq!(artifact.target, "rust");
        assert_eq!(
            artifact.path,
            fs::canonicalize(out.join("outputs/rust")).unwrap()
        );
        assert!(artifact.files.contains(&"Cargo.toml".into()));
        return;
    }
    if mode == "dependent" {
        let package = crucible_build::model_dependency("source_model");
        let artifact = package.rust_client("separate-rust").generate();
        assert!(
            fs::read_to_string(artifact.path.join("src/lib.rs"))
                .unwrap()
                .contains("WeatherClient")
        );
        assert_eq!(package.model_path(), root.join("model.json"));
        return;
    }
    if mode == "policy" {
        let old = fs::read(out.join("crucible-artifacts.json")).unwrap();
        let result = crucible_build::model_from_json("model.json")
            .policy("missing.toml")
            .try_compile_with(|_, _| panic!("AST must not invoke compiler"));
        assert!(result.is_err());
        assert_eq!(old, fs::read(out.join("crucible-artifacts.json")).unwrap());
        return;
    }
    if mode == "empty" {
        let package = crucible_build::model_from_json("model.json").compile();
        package.publish();
        let index: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("crucible-artifacts.json")).unwrap())
                .unwrap();
        assert_eq!(index["artifacts"], serde_json::json!([]));
        return;
    }
    let calls = Cell::new(0);
    let package = crucible_build::smithy("model").compile_with(|_, output| {
        calls.set(calls.get() + 1);
        fs::create_dir_all(output.join("crucible/model"))?;
        fs::write(output.join("crucible/model/model.json"), WEATHER)
    });
    package.publish();
    let rust = package.rust_client("rust").generate();
    let git = package
        .rust_client("rust-git")
        .runtime_git(
            "https://github.com/Operation-Leyline/crucible",
            &"a".repeat(40),
            ">=0.1.0, <0.2.0",
        )
        .generate();
    assert!(
        fs::read_to_string(git.path.join("Cargo.toml"))
            .unwrap()
            .contains("version = \">=0.1.0, <0.2.0\"")
    );
    assert!(
        fs::read_to_string(rust.path.join("src/lib.rs"))
            .unwrap()
            .contains("WeatherClient")
    );
    assert!(
        package
            .rust_client("rust")
            .try_generate()
            .unwrap_err()
            .contains("already configured")
    );
    assert!(
        package
            .rust_client("../escape")
            .try_generate()
            .unwrap_err()
            .contains("invalid output")
    );
    #[cfg(feature = "java")]
    {
        let java = package
            .java_client("java")
            .package("example.publicsdk")
            .group_id("example")
            .artifact_id("weather-public")
            .version("1.2.3")
            .generate();
        let private = package
            .java_client("java-private")
            .package("example.privatesdk")
            .artifact_id("weather-private")
            .generate();
        let pom = fs::read_to_string(java.path.join("pom.xml")).unwrap();
        assert!(pom.contains("weather-public") && pom.contains("1.2.3"));
        assert!(
            private
                .path
                .join("src/main/java/example/privatesdk/WeatherClient.java")
                .is_file()
        );
        let before = fs::read(java.path.join("pom.xml")).unwrap();
        // Another model generation session cannot replace a working native SDK
        // when its requested target configuration has diagnostics.
        let next =
            crucible_build::model_from_json("model.json").compile_with(|_, _| unreachable!());
        assert!(
            next.java_client("java")
                .package("not a Java package")
                .try_generate()
                .is_err()
        );
        assert_eq!(before, fs::read(java.path.join("pom.xml")).unwrap());
        // Restore this session's full registry after the failure probe.
        package
            .java_client("restored")
            .package("example.restored")
            .generate();
    }
    assert_eq!(
        calls.get(),
        1,
        "all SDK variants share a single Smithy compilation"
    );
}

fn probe(root: &std::path::Path, mode: &str) -> String {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "package_probe", "--nocapture"])
        .env("CRUCIBLE_SDK_PROBE", mode)
        .env("CARGO_MANIFEST_DIR", root)
        .env("OUT_DIR", root.join("build-output"))
        .env("DEP_SOURCE_MODEL_MODEL", root.join("model.json"))
        .env(
            "DEP_SOURCE_MODEL_ARTIFACT_RUST",
            fs::canonicalize(root.join("build-output/outputs/rust"))
                .unwrap_or_else(|_| root.join("build-output/outputs/rust")),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn one_compile_publishes_named_variants_with_stable_sources_and_explicit_export() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir(root.join("model")).unwrap();
    fs::write(root.join("model/api.smithy"), "// compiler seam fixture").unwrap();
    fs::write(root.join("model.json"), WEATHER).unwrap();
    let stdout = probe(root, "producer");
    assert!(
        stdout.contains("cargo::metadata=model=")
            && stdout.contains("cargo::metadata=artifact_rust=")
    );
    let source = root.join("build-output/outputs/rust/src/lib.rs");
    let modified = fs::metadata(&source).unwrap().modified().unwrap();
    probe(root, "producer");
    assert_eq!(modified, fs::metadata(source).unwrap().modified().unwrap());
    let index = root.join("build-output/crucible-artifacts.json");
    probe(root, "resolve");
    let export = root.join("export/rust");
    crucible_build::export_artifact(&index, "rust", &export).unwrap();
    assert!(export.join("src/lib.rs").is_file());
    let before = fs::read(export.join("src/lib.rs")).unwrap();
    assert!(crucible_build::export_artifact(&index, "unknown", &export).is_err());
    assert_eq!(before, fs::read(export.join("src/lib.rs")).unwrap());
    probe(root, "policy");
    probe(root, "empty");
    assert!(crucible_build::export_artifact(&index, "rust", &export).is_err());
}

#[test]
fn a_separate_sdk_package_generates_from_its_model_dependency_without_compilation() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("model.json"), WEATHER).unwrap();
    let stdout = probe(directory.path(), "dependent");
    assert!(stdout.contains("cargo::metadata=artifact_separate_rust="));
}

#[test]
fn artifact_export_rejects_paths_outside_the_generated_package_before_writing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir(root.join("source")).unwrap();
    fs::create_dir(root.join("export")).unwrap();
    fs::write(root.join("private"), "private").unwrap();
    fs::write(root.join("export/keep"), "keep").unwrap();
    let artifact = crucible_build::Artifact {
        name: "java".into(),
        target: "java".into(),
        path: root.join("source"),
        files: vec!["../private".into()],
    };
    assert!(artifact.export(root.join("export")).is_err());
    assert_eq!(
        fs::read_to_string(root.join("export/keep")).unwrap(),
        "keep"
    );
    assert!(!root.join("export/private").exists());
}

#[test]
fn named_outputs_default_external_and_can_select_internal() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("model.json"),
        include_str!("../../crucible-smithy/tests/fixtures/internal.json"),
    )
    .unwrap();
    probe(root.path(), "audience");
}
