use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crucible_codegen_rust::{
    ClientOptions, File, RuntimeDependency, emit_client_package, generate_client,
};
use crucible_smithy::Model;

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

fn service() -> crucible_smithy::ir::Service {
    Model::from_json(WEATHER).unwrap().lower(None).unwrap()
}

fn explicit() -> ClientOptions {
    ClientOptions {
        package_name: Some("weather-sdk".into()),
        package_version: "1.2.3".into(),
        runtime: RuntimeDependency::Git {
            repository: "https://github.com/Operation-Leyline/crucible".into(),
            revision: "0123456789abcdef0123456789abcdef01234567".into(),
        },
    }
}

fn successful(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn save(root: &Path, files: &[File]) {
    for file in files {
        let path = root.join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
}

fn golden(name: &str, text: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/package")
        .join(name);
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
    }
    assert_eq!(
        text,
        fs::read_to_string(path).expect("native Rust package golden exists")
    );
}

#[test]
fn native_package_manifest_and_public_api_match_explicit_source_goldens() {
    let service = service();
    let files = emit_client_package(&service, &explicit()).unwrap();
    assert_eq!(
        files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["Cargo.toml", "src/lib.rs"]
    );
    golden("Cargo.toml", &files[0].contents);
    let source = generate_client(&service).unwrap();
    assert!(files[1].contents.starts_with(&source));
    golden("runtime_api.rs", &files[1].contents[source.len()..]);
    assert!(!files[1].contents.contains("include!"));
    assert!(!files.iter().any(|file| file.path == "build.rs"));
    let manifest: toml::Value = toml::from_str(&files[0].contents).unwrap();
    assert_eq!(manifest["package"]["name"].as_str(), Some("weather-sdk"));
    assert_eq!(manifest["package"]["version"].as_str(), Some("1.2.3"));
    assert!(manifest["workspace"].is_table());
}

#[test]
fn automatic_runtime_pins_the_embedded_crucible_commit_without_local_paths() {
    let files = emit_client_package(&service(), &ClientOptions::default()).unwrap();
    let manifest: toml::Value = toml::from_str(&files[0].contents).unwrap();
    assert_eq!(manifest["package"]["name"].as_str(), Some("weather-sdk"));
    let dependency = &manifest["dependencies"]["crucible-client-runtime-rust"];
    assert_eq!(
        dependency["git"].as_str(),
        Some("https://github.com/Operation-Leyline/crucible")
    );
    assert!(dependency.get("path").is_none());
    let revision = dependency["rev"].as_str().unwrap();
    assert_eq!(revision.len(), 40);
    assert!(revision.bytes().all(|c| c.is_ascii_hexdigit()));
    let output = successful(
        Command::new("git")
            .arg("-C")
            .arg(env!("CARGO_MANIFEST_DIR"))
            .args(["cat-file", "-t", revision])
            .output()
            .unwrap(),
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "commit");
}

#[test]
fn options_are_diagnosed_before_any_package_files_are_returned() {
    let mut options = explicit();
    options.package_name = Some("../bad package".into());
    options.package_version = "1".into();
    options.runtime = RuntimeDependency::Git {
        repository: "not a URL".into(),
        revision: "main".into(),
    };
    let diagnostics = emit_client_package(&service(), &options).unwrap_err();
    assert_eq!(diagnostics.len(), 3);
    for name in ["", "_", "fn", "self", "invalid.name", "0invalid"] {
        options = explicit();
        options.package_name = Some(name.into());
        assert!(emit_client_package(&service(), &options).is_err(), "{name}");
    }
    for dependency in [
        RuntimeDependency::Git {
            repository: "".into(),
            revision: "main".into(),
        },
        RuntimeDependency::Git {
            repository: "https://example.test/repo".into(),
            revision: "\nmain".into(),
        },
        RuntimeDependency::Path(PathBuf::from("relative/runtime")),
    ] {
        options = explicit();
        options.runtime = dependency;
        assert!(emit_client_package(&service(), &options).is_err());
    }
    let directory = tempfile::tempdir().unwrap();
    options.runtime = RuntimeDependency::Path(directory.path().into());
    assert!(emit_client_package(&service(), &options).is_err());
    fs::write(
        directory.path().join("Cargo.toml"),
        "[package]\nname = 'unrelated'\n",
    )
    .unwrap();
    assert!(emit_client_package(&service(), &options).is_err());
}

#[test]
fn local_paths_and_git_urls_roundtrip_through_quoted_toml() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("runtime 'quoted'");
    fs::create_dir(&path).unwrap();
    fs::write(
        path.join("Cargo.toml"),
        "[package]\nname = 'crucible-client-runtime-rust'\n",
    )
    .unwrap();
    let mut options = explicit();
    options.runtime = RuntimeDependency::Path(path.clone());
    let files = emit_client_package(&service(), &options).unwrap();
    let manifest: toml::Value = toml::from_str(&files[0].contents).unwrap();
    assert_eq!(
        manifest["dependencies"]["crucible-client-runtime-rust"]["path"].as_str(),
        path.to_str()
    );
    let repository = "https://example.test/\"quoted\".git";
    options.runtime = RuntimeDependency::Git {
        repository: repository.into(),
        revision: "test-reference".into(),
    };
    let files = emit_client_package(&service(), &options).unwrap();
    let manifest: toml::Value = toml::from_str(&files[0].contents).unwrap();
    assert_eq!(
        manifest["dependencies"]["crucible-client-runtime-rust"]["git"].as_str(),
        Some(repository)
    );
}

#[test]
fn operation_names_do_not_collide_with_runtime_shortcuts() {
    for (operation, shortcut) in [("Config", "Config"), ("Waiter", "WaiterError")] {
        let mut service = service();
        service.operations[0].id = crucible_smithy::ShapeId::new("example.weather", operation);
        let files = emit_client_package(&service, &explicit()).unwrap();
        let exports = files[1]
            .contents
            .rsplit_once("pub use ::crucible_client_runtime_rust::{")
            .unwrap()
            .1;
        assert!(
            !exports
                .split([',', '}', ';', ' '])
                .any(|name| name == shortcut)
        );
        assert!(
            files[1]
                .contents
                .contains("pub use ::crucible_client_runtime_rust as runtime;")
        );
    }
}

#[test]
fn an_independent_cargo_application_depends_only_on_the_generated_sdk() {
    let directory = tempfile::tempdir().unwrap();
    let sdk = directory.path().join("sdk");
    let consumer = directory.path().join("consumer");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crucible-client-runtime-rust")
        .canonicalize()
        .unwrap();
    let mut options = explicit();
    options.runtime = RuntimeDependency::Path(runtime);
    save(&sdk, &emit_client_package(&service(), &options).unwrap());
    assert!(!sdk.join("build.rs").exists());
    fs::create_dir_all(consumer.join("src")).unwrap();
    fs::write(consumer.join("Cargo.toml"), "[package]\nname = 'independent-sdk-consumer'\nversion = '0.1.0'\nedition = '2024'\n[dependencies]\nweather-sdk = { path = '../sdk' }\n[workspace]\n").unwrap();
    fs::write(consumer.join("src/main.rs"), r#"
use weather_sdk::{CallOptions, Config, Error, GetCityError, Token, WeatherClient, model::GetCityInput, runtime};
fn main() {
    let client = WeatherClient::new(Config::new("http://localhost:8080").token(Token::fixed("secret")));
    let future = client.get_city(GetCityInput::builder().city_id("seattle").build());
    drop(future);
    let _: Option<Error<GetCityError>> = None;
    let _ = CallOptions::default();
    let _ = runtime::CancellationToken::new();
}
"#).unwrap();
    let target =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.codex-target/rust-native-consumer");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = successful(
        Command::new(&cargo)
            .arg("metadata")
            .args(["--format-version", "1", "--no-deps", "--manifest-path"])
            .arg(sdk.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .unwrap(),
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(metadata["packages"][0]["name"], "weather-sdk");
    assert_eq!(
        metadata["packages"][0]["dependencies"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    successful(
        Command::new(&cargo)
            .arg("run")
            .arg("--manifest-path")
            .arg(consumer.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_BUILD_JOBS", "1")
            .output()
            .unwrap(),
    );
    let _ = successful(
        Command::new(&cargo)
            .args(["package", "--list", "--allow-dirty", "--manifest-path"])
            .arg(sdk.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .unwrap(),
    );
}
