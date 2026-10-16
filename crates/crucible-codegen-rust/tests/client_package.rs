#[path = "../../../tests/support/cargo_target.rs"]
mod cargo_target;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crucible_codegen_rust::{
    ClientOptions, File, RuntimeDependency, emit_client_package, generate_client,
};
mod common;

fn service() -> crucible_ir::ir::Service {
    common::load("weather")
}

fn explicit() -> ClientOptions {
    ClientOptions {
        package_name: Some("weather-sdk".into()),
        package_version: "1.2.3".into(),
        runtime: RuntimeDependency::Git {
            repository: "https://github.com/Operation-Leyline/crucible".into(),
            revision: "0123456789abcdef0123456789abcdef01234567".into(),
            version_requirement: "9.8.7".into(),
        },
        metadata: Default::default(),
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
        [
            "Cargo.toml",
            "src/lib.rs",
            ".rustfmt.toml",
            "README.md",
            "AGENTS.md"
        ]
    );
    golden("Cargo.toml", &files[0].contents);
    let source = generate_client(&service).unwrap();
    // The README is the crate's documentation, generated where none is
    // configured.
    let docs = "#![cfg_attr(not(doctest), doc = include_str!(\"../README.md\"))]

";
    let api = files[1].contents.strip_prefix(docs).unwrap();
    assert!(api.starts_with(&source));
    golden("runtime_api.rs", &api[source.len()..]);
    golden("rustfmt.toml", &files[2].contents);
    golden("README.md", &files[3].contents);
    golden("AGENTS.md", &files[4].contents);
    assert!(!files[1].contents.contains("include!"));
    assert!(!files.iter().any(|file| file.path == "build.rs"));
    let manifest: toml::Value = toml::from_str(&files[0].contents).unwrap();
    assert_eq!(manifest["package"]["name"].as_str(), Some("weather-sdk"));
    assert_eq!(manifest["package"]["version"].as_str(), Some("1.2.3"));
    assert!(manifest["workspace"].is_table());
    assert_eq!(
        manifest["dependencies"]["crucible-client-runtime-rust"]["version"].as_str(),
        Some("9.8.7")
    );
}

#[test]
fn automatic_runtime_pins_the_embedded_crucible_commit_without_local_paths() {
    let files = emit_client_package(&service(), &ClientOptions::default()).unwrap();
    let manifest: toml::Value = toml::from_str(&files[0].contents).unwrap();
    assert_eq!(manifest["package"]["name"].as_str(), Some("weather-sdk"));
    let dependency = &manifest["dependencies"]["crucible-client-runtime-rust"];
    assert_eq!(
        dependency["version"].as_str(),
        Some(env!("CARGO_PKG_VERSION"))
    );
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
        version_requirement: "9.8.7".into(),
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
            version_requirement: "9.8.7".into(),
        },
        RuntimeDependency::Git {
            repository: "https://example.test/repo".into(),
            revision: "\nmain".into(),
            version_requirement: "9.8.7".into(),
        },
        RuntimeDependency::Git {
            repository: "https://example.test/repo".into(),
            revision: "main".into(),
            version_requirement: "not a version".into(),
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
        "[package]\nname = 'unrelated'\nversion = '9.8.7'\n[lib]\npath = 'lib.rs'\n",
    )
    .unwrap();
    fs::write(directory.path().join("lib.rs"), "").unwrap();
    let diagnostics = emit_client_package(&service(), &options).unwrap_err();
    assert!(
        diagnostics
            .to_string()
            .contains("must name the crucible-client-runtime-rust")
    );
}

#[test]
fn local_paths_and_git_urls_roundtrip_through_quoted_toml() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("runtime 'quoted'");
    fs::create_dir(&path).unwrap();
    fs::write(
        path.join("Cargo.toml"),
        "[package]\nname = 'crucible-client-runtime-rust'\nversion = '9.8.7'\n[lib]\npath = 'lib.rs'\n",
    )
    .unwrap();
    let mut options = explicit();
    options.runtime = RuntimeDependency::Path(path.clone());
    fs::write(path.join("lib.rs"), "").unwrap();
    let files = emit_client_package(&service(), &options).unwrap();
    let manifest: toml::Value = toml::from_str(&files[0].contents).unwrap();
    assert_eq!(
        manifest["dependencies"]["crucible-client-runtime-rust"]["path"].as_str(),
        path.to_str()
    );
    assert_eq!(
        manifest["dependencies"]["crucible-client-runtime-rust"]["version"].as_str(),
        Some("9.8.7")
    );
    let repository = "https://example.test/\"quoted\".git";
    options.runtime = RuntimeDependency::Git {
        repository: repository.into(),
        revision: "test-reference".into(),
        version_requirement: ">=9.8.7, <10.0.0".into(),
    };
    let files = emit_client_package(&service(), &options).unwrap();
    let manifest: toml::Value = toml::from_str(&files[0].contents).unwrap();
    assert_eq!(
        manifest["dependencies"]["crucible-client-runtime-rust"]["git"].as_str(),
        Some(repository)
    );
    assert_eq!(
        manifest["dependencies"]["crucible-client-runtime-rust"]["version"].as_str(),
        Some(">=9.8.7, <10.0.0")
    );
}

#[test]
fn local_runtime_uses_its_effective_workspace_version_without_building() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    fs::create_dir(&runtime).unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        "[workspace]\nmembers = ['runtime']\n[workspace.package]\nversion = '9.8.7'\n",
    )
    .unwrap();
    fs::write(runtime.join("Cargo.toml"), "[package]\nname = 'crucible-client-runtime-rust'\nversion.workspace = true\nbuild = 'build.rs'\n[lib]\npath = 'lib.rs'\n[dependencies]\nnever-fetched = { git = 'https://example.invalid/metadata-must-not-fetch' }\n").unwrap();
    fs::write(runtime.join("lib.rs"), "").unwrap();
    fs::write(
        runtime.join("build.rs"),
        "compile_error!(\"metadata must not build the runtime\");",
    )
    .unwrap();
    let mut options = explicit();
    options.runtime = RuntimeDependency::Path(runtime);
    let files = emit_client_package(&service(), &options).unwrap();
    let manifest: toml::Value = toml::from_str(&files[0].contents).unwrap();
    assert_eq!(
        manifest["dependencies"]["crucible-client-runtime-rust"]["version"].as_str(),
        Some("9.8.7")
    );
    assert!(!directory.path().join("Cargo.lock").exists());
    assert!(!directory.path().join("target").exists());
}

#[test]
fn invalid_local_runtime_workspace_is_a_cargo_metadata_diagnostic() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("Cargo.toml"), "[package]\nname = 'crucible-client-runtime-rust'\nversion.workspace = true\n[lib]\npath = 'lib.rs'\n").unwrap();
    fs::write(directory.path().join("lib.rs"), "").unwrap();
    let mut options = explicit();
    options.runtime = RuntimeDependency::Path(directory.path().into());
    let diagnostics = emit_client_package(&service(), &options).unwrap_err();
    assert!(
        diagnostics
            .to_string()
            .contains("cannot read local runtime Cargo metadata")
    );
}

#[test]
fn operation_names_do_not_collide_with_runtime_shortcuts() {
    for (operation, shortcut) in [("Config", "Config"), ("Retry", "Retry")] {
        let mut service = service();
        service.operations[0].id = crucible_ir::ShapeId::new("example.weather", operation);
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
fn model_and_exception_names_take_precedence_over_runtime_shortcuts() {
    for (shape, shortcut) in [
        ("CityId", "Config"),
        ("NotFoundError", "Error"),
        ("Location", "Field"),
    ] {
        let mut service = service();
        common::rename(
            &mut service,
            &[(&format!("example.weather#{shape}"), shortcut)],
        );
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
    let documented = common::load_with("weather", |package| {
        for file in &mut package.files {
            file.text = file
                .text
                .replace(
                    "        location?: Location\n",
                    "        location?: Location\n        debugModel?: DebugModel\n",
                )
                .replace(
                    "error NotFoundError {\n    status 404\n    message: string",
                    "error NotFoundError {\n    status 404\n    sensitive message: string",
                );
        }
        package.files.push(crucible_lang::SourceFile {
            path: "native-docs.cru".into(),
            text: include_str!("../../../tests/conformance/native-docs.cru").into(),
        });
    });
    save(&sdk, &emit_client_package(&documented, &options).unwrap());
    let mut renamed = service();
    common::rename(
        &mut renamed,
        &[
            ("example.weather#CityId", "Config"),
            ("example.weather#NotFoundError", "Error"),
            ("example.weather#Location", "Field"),
        ],
    );
    let mut renamed_options = options.clone();
    renamed_options.package_name = Some("renamed-weather-sdk".into());
    save(
        &directory.path().join("renamed-sdk"),
        &emit_client_package(&renamed, &renamed_options).unwrap(),
    );
    assert!(!sdk.join("build.rs").exists());
    fs::create_dir_all(consumer.join("src")).unwrap();
    fs::write(consumer.join("Cargo.toml"), "[package]\nname = 'independent-sdk-consumer'\nversion = '0.1.0'\nedition = '2024'\n[dependencies]\nweather-sdk = { path = '../sdk' }\nrenamed-weather-sdk = { path = '../renamed-sdk' }\n[workspace]\n").unwrap();
    fs::write(consumer.join("src/main.rs"), r#"
use weather_sdk::{CallOptions, Config, Error, GetCityError, Token, WeatherClient, GetCityInput, runtime};
fn main() {
    let nested = weather_sdk::DebugNested::builder().label("visible").secret("SECRET-NESTED").build();
    let builder = weather_sdk::DebugModel::builder().label("visible").secret("SECRET-MEMBER").ticket("SECRET-TARGET")
        .nested(nested.clone()).items(vec![nested]).values(vec!["SECRET-ELEMENT".to_owned()])
        .choice(weather_sdk::DebugChoice::Secret("SECRET-UNION".to_owned())).enumeration(weather_sdk::DebugEnum::Known);
    let text = format!("{builder:?}");
    assert!(text.contains("visible") && text.contains("Sensitive Data Redacted") && !text.contains("SECRET-"));
    let model = builder.build();
    let text = format!("{model:?}");
    assert!(text.contains("DebugNested") && !text.contains("SECRET-"));
    assert!(!format!("{:?}", weather_sdk::DebugEnum::UnknownValue("SECRET-ENUM".to_owned())).contains("SECRET-"));
    let error = weather_sdk::NotFoundError::builder().message("SECRET-ERROR").build().unwrap();
    assert!(!format!("{error:?} {error}").contains("SECRET-"));
    let client = WeatherClient::new(Config::new("http://localhost:8080").token(Token::fixed("secret")));
    let future = client.get_city(GetCityInput::builder().city_id("seattle").build().unwrap());
    drop(future);
    let _: Option<Error<GetCityError>> = None;
    let _ = CallOptions::default();
    let _ = runtime::CancellationToken::new();
    let id: renamed_weather_sdk::Config = "seattle".into();
    let client = renamed_weather_sdk::WeatherClient::new(renamed_weather_sdk::runtime::Config::new("http://localhost:8080"));
    drop(client.get_city(renamed_weather_sdk::GetCityInput::builder().city_id(id).build().unwrap()));
    let _: Option<renamed_weather_sdk::Error> = None;
    let _: Option<renamed_weather_sdk::Field> = None;
    let _: Option<renamed_weather_sdk::runtime::Error<renamed_weather_sdk::GetCityError>> = None;
}
"#).unwrap();
    let target = cargo_target::target_dir();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    successful(
        Command::new(&cargo)
            .args(["fmt", "--manifest-path"])
            .arg(consumer.join("Cargo.toml"))
            .output()
            .unwrap(),
    );
    let generated_source = fs::read(sdk.join("src/lib.rs")).unwrap();
    let formatting = fs::read(sdk.join(".rustfmt.toml")).unwrap();
    fs::remove_file(sdk.join(".rustfmt.toml")).unwrap();
    let without_config = Command::new(&cargo)
        .args(["fmt", "--all", "--manifest-path"])
        .arg(consumer.join("Cargo.toml"))
        .args(["--", "--check"])
        .output()
        .unwrap();
    assert!(
        !without_config.status.success(),
        "cargo fmt must visit the unformatted generated SDK"
    );
    fs::write(sdk.join(".rustfmt.toml"), &formatting).unwrap();
    let formatted = successful(
        Command::new(&cargo)
            .args(["fmt", "--all", "--manifest-path"])
            .arg(consumer.join("Cargo.toml"))
            .args(["--", "--check"])
            .output()
            .unwrap(),
    );
    assert!(
        formatted.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&formatted.stderr)
    );
    assert_eq!(generated_source, fs::read(sdk.join("src/lib.rs")).unwrap());
    assert_eq!(formatting, fs::read(sdk.join(".rustfmt.toml")).unwrap());
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
    successful(
        Command::new(&cargo)
            .args(["doc", "--no-deps", "--manifest-path"])
            .arg(sdk.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target)
            .env("RUSTDOCFLAGS", "-D warnings")
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

/// Common package metadata maps onto `Cargo.toml` as docs/sdk-packaging.md's
/// table says; the README becomes the crate's documentation.
#[test]
fn package_metadata_matches_golden() {
    let mut options = explicit();
    let metadata = &mut options.metadata;
    metadata.description = Some("Client for the \"Weather\" API.".into());
    metadata.repository = Some("https://github.com/example/weather".into());
    metadata.homepage = Some("https://weather.example".into());
    metadata.documentation = Some("https://docs.rs/weather-sdk".into());
    metadata.license = Some("MIT OR Apache-2.0".into());
    metadata.authors = vec!["Weather Team <sdk@weather.example>".into()];
    metadata.readme = Some("# Weather\n".into());
    metadata.keywords = vec!["weather".into(), "forecast".into()];
    metadata.private = true;
    let files = emit_client_package(&service(), &options).unwrap();
    golden("Cargo-metadata.toml", &files[0].contents);
    assert!(
        files[1]
            .contents
            .starts_with("#![cfg_attr(not(doctest), doc = include_str!(\"../README.md\"))]\n\n// Generated by crucible")
    );
    let readme = files.iter().find(|file| file.path == "README.md").unwrap();
    assert_eq!(readme.contents, "# Weather\n");
    assert!(!files.iter().any(|file| file.path == "LICENSE"));
    let manifest: toml::Value = toml::from_str(&files[0].contents).unwrap();
    assert_eq!(manifest["package"]["publish"].as_bool(), Some(false));

    options.metadata.license = None;
    options.metadata.license_file = Some("Proprietary.\n".into());
    let files = emit_client_package(&service(), &options).unwrap();
    let manifest: toml::Value = toml::from_str(&files[0].contents).unwrap();
    assert_eq!(
        manifest["package"]["license-file"].as_str(),
        Some("LICENSE")
    );
    assert!(manifest["package"].get("license").is_none());
    let license = files.iter().find(|file| file.path == "LICENSE").unwrap();
    assert_eq!(license.contents, "Proprietary.\n");
}

#[test]
fn metadata_crates_io_would_refuse_is_a_diagnostic() {
    let mut options = explicit();
    options.metadata.keywords = vec!["9lives".into(), "a".repeat(21)];
    options.metadata.homepage = Some("weather.example".into());
    let error = emit_client_package(&service(), &options)
        .unwrap_err()
        .to_string();
    assert!(error.contains("\"9lives\""), "{error}");
    assert!(
        error.contains(&format!("\"{}\"", "a".repeat(21))),
        "{error}"
    );
    assert!(error.contains("`homepage`"), "{error}");
    options.metadata = Default::default();
    options.metadata.registry = Some(crucible_codegen_common::Registry::npm(
        "https://registry.npmjs.org",
        None,
    ));
    assert!(emit_client_package(&service(), &options).is_err());
    // A published URL never carries credentials, and the error never repeats them.
    options.metadata = Default::default();
    options.metadata.repository = Some("https://user:sentinel-secret@github.com/x".into());
    let error = emit_client_package(&service(), &options)
        .unwrap_err()
        .to_string();
    assert!(error.contains("`repository`"), "{error}");
    assert!(!error.contains("sentinel-secret"), "{error}");
}

/// docs/sdk-packaging.md's table: a Git channel is `publish = false`,
/// whether or not the SDK is private; otherwise only `private` adds it.
#[test]
fn a_git_channel_or_private_package_is_never_published_from_its_manifest() {
    let publish = |private: bool, registry: Option<crucible_codegen_common::Registry>| {
        let mut options = explicit();
        options.metadata.private = private;
        options.metadata.registry = registry;
        let files = emit_client_package(&service(), &options).unwrap();
        let manifest: toml::Value = toml::from_str(&files[0].contents).unwrap();
        manifest["package"]
            .get("publish")
            .map(|value| value.as_bool())
    };
    let git = Some(crucible_codegen_common::Registry::Git);
    assert_eq!(publish(false, git.clone()), Some(Some(false)));
    assert_eq!(publish(true, git), Some(Some(false)));
    assert_eq!(publish(true, None), Some(Some(false)));
    assert_eq!(publish(false, None), None);
}
