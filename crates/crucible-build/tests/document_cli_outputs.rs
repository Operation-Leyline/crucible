#![cfg(all(feature = "openapi", feature = "cli"))]

#[path = "../../../tests/support/cargo_target.rs"]
mod cargo_target;
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crucible_build::{CliOptions, Generator, OpenApiOptions};
use serde_json::Value;

fn checkout() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn document_cli_probe() {
    let Ok(mode) = std::env::var("CRUCIBLE_DOCUMENT_CLI_PROBE") else {
        return;
    };
    let model = crucible_build::model("weather").compile();
    if mode == "cli-consumer" {
        model.cli("weather").generate_into("exports/cli");
        return;
    }

    let endpoint = "https://weather.example/api";
    let document = model
        .openapi("openapi")
        .options(OpenApiOptions::default())
        .endpoint(endpoint)
        .generate_into("exports/openapi");
    assert_eq!(document.target, "openapi");
    assert_eq!(document.files, ["openapi.json"]);
    let service = crucible_lang::load_service(&common::shared_package("weather")).unwrap();
    let expected = Generator::OpenApi(OpenApiOptions {
        endpoint: Some(endpoint.into()),
    })
    .emit(&service)
    .unwrap();
    assert_eq!(
        fs::read_to_string(document.path.join("openapi.json")).unwrap(),
        expected[0].contents
    );
    let exported_document = fs::read("exports/openapi/openapi.json").unwrap();
    let json: Value = serde_json::from_slice(&exported_document).unwrap();
    assert_eq!(json["servers"][0]["url"], endpoint);
    assert!(json["paths"]["/cities"]["post"].is_object());

    let default_cli = model.cli("weather").generate_into("exports/cli");
    assert_eq!(default_cli.target, "cli");
    for file in ["Cargo.toml", "schema.json", "src/client.rs", "src/main.rs"] {
        assert!(default_cli.path.join(file).is_file(), "{file}");
    }
    let manifest = fs::read_to_string("exports/cli/Cargo.toml").unwrap();
    assert!(manifest.contains("name = \"weather-cli\""), "{manifest}");
    assert!(manifest.contains("version = \"4.5.6\""), "{manifest}");
    for runtime in ["crucible-client-runtime-rust", "crucible-cli-runtime"] {
        let expected = checkout()
            .join("crates")
            .join(runtime)
            .to_string_lossy()
            .replace('\\', "/");
        assert!(manifest.contains(&expected), "{manifest}");
    }

    let explicit = model
        .cli("explicit")
        .options(CliOptions {
            bin_name: "initial".into(),
            package_version: "0.1.0".into(),
            endpoint: None,
            runtime_path: Some(checkout()),
        })
        .package("weather-tools")
        .bin_name("weather-admin")
        .version("2.3.4")
        .endpoint(endpoint)
        .runtime_path(checkout())
        .try_generate()
        .unwrap();
    let schema: Value =
        serde_json::from_slice(&fs::read(explicit.path.join("schema.json")).unwrap()).unwrap();
    assert_eq!(schema["name"], "weather-admin");
    assert_eq!(schema["version"], "2.3.4");
    assert_eq!(schema["endpoint"], endpoint);

    let relative = model
        .cli("relative")
        .runtime_path("runtime")
        .try_generate()
        .unwrap();
    let relative_manifest = fs::read_to_string(relative.path.join("Cargo.toml")).unwrap();
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    // The generator canonicalizes the runtime path, so the expectation must
    // too: on Windows TEMP is `C:\WINDOWS\TEMP`, whose canonical spelling is
    // `C:\Windows\Temp`. The manifest spells it as Cargo does, with `/` and
    // without the verbatim `\\?\` prefix, so `\\?\UNC\host\share` is
    // `//host/share`.
    let runtime = root.join("runtime").canonicalize().unwrap();
    let runtime = runtime.to_string_lossy().replace('\\', "/");
    let runtime = match runtime.strip_prefix("//?/UNC/") {
        Some(unc) => format!("//{unc}"),
        None => runtime.strip_prefix("//?/").unwrap_or(&runtime).to_owned(),
    };
    assert!(relative_manifest.contains(&runtime), "{relative_manifest}");
    let advanced_relative = model
        .cli("advanced-relative")
        .options(CliOptions {
            bin_name: "advanced-cli".into(),
            package_version: "1.2.3".into(),
            endpoint: None,
            runtime_path: Some("runtime".into()),
        })
        .generate();
    assert!(advanced_relative.path.join("Cargo.toml").is_file());

    for (path, expected) in [
        ("missing-runtime", "cannot resolve runtime path"),
        (
            "incomplete-runtime",
            "missing crates/crucible-cli-runtime/Cargo.toml",
        ),
    ] {
        let error = model
            .cli("invalid-runtime")
            .runtime_path(path)
            .try_generate_into("exports/cli")
            .unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert_eq!(
            manifest,
            fs::read_to_string("exports/cli/Cargo.toml").unwrap()
        );
    }

    let next = crucible_build::model("weather").compile();
    let error = next
        .cli("weather")
        .version("invalid version")
        .try_generate_into("exports/cli")
        .unwrap_err();
    assert!(
        error.contains("CLI package version must be a semantic version"),
        "{error}"
    );
    assert_eq!(
        manifest,
        fs::read_to_string("exports/cli/Cargo.toml").unwrap()
    );
}

fn probe(root: &Path, mode: &str) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "document_cli_probe", "--nocapture"])
        .env("CRUCIBLE_DOCUMENT_CLI_PROBE", mode)
        .env("CARGO_MANIFEST_DIR", root)
        .env("CARGO_PKG_VERSION", "4.5.6")
        .env("OUT_DIR", root.join("out"))
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn documents_and_cli_packages_share_native_defaults_exports_and_diagnostics() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    common::install_shared(root, "weather");
    for (checkout, crates) in [
        (
            "runtime",
            vec!["crucible-client-runtime-rust", "crucible-cli-runtime"],
        ),
        ("incomplete-runtime", vec!["crucible-client-runtime-rust"]),
    ] {
        for name in crates {
            let path = root.join(checkout).join("crates").join(name);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("Cargo.toml"), "[package]\n").unwrap();
        }
    }
    probe(root, "outputs");
}

#[test]
fn generated_cli_is_warning_free_as_an_independent_offline_cargo_package() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    common::install_shared(root, "weather");
    probe(root, "cli-consumer");
    // Reuse verified registry versions while resolving the generated package's
    // native path runtimes; the consumer sources stay isolated while dependencies share a build cache.
    fs::copy(
        checkout().join("Cargo.lock"),
        root.join("exports/cli/Cargo.lock"),
    )
    .unwrap();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let manifest = root.join("exports/cli/Cargo.toml");
    let source_paths = ["src/client.rs", "src/lib.rs", "src/main.rs"];
    let sources: Vec<_> = source_paths
        .iter()
        .map(|path| fs::read(root.join("exports/cli").join(path)).unwrap())
        .collect();
    let config_path = root.join("exports/cli/.rustfmt.toml");
    let config = fs::read(&config_path).unwrap();
    fs::remove_file(&config_path).unwrap();
    let without_config = Command::new(&cargo)
        .args(["fmt", "--all", "--manifest-path"])
        .arg(&manifest)
        .args(["--", "--check"])
        .output()
        .unwrap();
    assert!(
        !without_config.status.success(),
        "cargo fmt must check the generated CLI sources"
    );
    fs::write(&config_path, &config).unwrap();
    let formatting = Command::new(&cargo)
        .args(["fmt", "--all", "--manifest-path"])
        .arg(&manifest)
        .args(["--", "--check"])
        .output()
        .unwrap();
    assert!(
        formatting.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&formatting.stdout),
        String::from_utf8_lossy(&formatting.stderr)
    );
    assert!(
        formatting.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&formatting.stderr)
    );
    for (path, before) in source_paths.iter().zip(sources) {
        assert_eq!(
            before,
            fs::read(root.join("exports/cli").join(path)).unwrap()
        );
    }
    assert_eq!(config, fs::read(config_path).unwrap());
    let output = Command::new(cargo)
        .args(["clippy", "--offline", "--all-targets", "--manifest-path"])
        .arg(&manifest)
        .args(["--", "-D", "warnings"])
        .env("CARGO_TARGET_DIR", cargo_target::target_dir())
        .env("CARGO_BUILD_JOBS", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
