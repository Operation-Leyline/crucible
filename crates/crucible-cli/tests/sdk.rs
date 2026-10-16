//! `crucible sdk config|generate`, and the package metadata flags of
//! `crucible generate rust|ts` (docs/sdk-packaging.md).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

mod common;

const SDK_FILE: &str = r#"schema_version = 1
model = "weather"

[defaults]
description = "Client for the Weather API."
repository = "https://github.com/example/weather"
license = "MIT"
authors = ["Weather <sdk@weather.example>"]
readme = "sdk/README.md"
private = true

[sdk.ts]
package = "@example/weather"
endpoint = { env = "WEATHER_SDK_TEST_ENDPOINT" }
out = "generated/ts"
channel = { npm = "https://npm.pkg.github.com", access = "restricted" }

[sdk.rust]
package = "weather-sdk"
version = "0.2.0"
out = "generated/rust"
keywords = ["weather"]
runtime = { git = "https://github.com/Operation-Leyline/crucible", rev = "0123456789abcdef0123456789abcdef01234567", version = "0.6" }
channel = { git = "git@github.com:example/weather-rs.git" }
"#;

fn crucible(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_crucible"))
        .current_dir(directory)
        .env("WEATHER_SDK_TEST_ENDPOINT", "https://api.weather.example")
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn failure(output: Output) -> String {
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8(output.stderr).unwrap()
}

/// A service repository with an SDK file, a model and a README.
fn service(sdk_file: &str) -> tempfile::TempDir {
    let directory = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let root = directory.path();
    fs::create_dir(root.join(".git")).unwrap();
    common::install_shared(root, "weather");
    fs::create_dir(root.join("sdk")).unwrap();
    fs::write(root.join("sdk/README.md"), "# Weather\r\n\r\nThe SDK.\r\n").unwrap();
    fs::write(root.join("crucible-sdk.toml"), sdk_file).unwrap();
    directory
}

fn json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn config_prints_each_sdks_effective_configuration() {
    let directory = service(SDK_FILE);
    let nested = directory.path().join("sdk");
    // Found from a subdirectory, as in a service repository.
    let config: serde_json::Value =
        serde_json::from_str(&success(crucible(&nested, &["sdk", "config", "--json"]))).unwrap();
    assert_eq!(config["model"], "weather");
    let ts = &config["sdks"]["ts"];
    assert_eq!(ts["endpoint"], "https://api.weather.example");
    assert_eq!(ts["endpoint_env"], "WEATHER_SDK_TEST_ENDPOINT");
    assert_eq!(ts["description"], "Client for the Weather API.");
    assert_eq!(ts["private"], true);
    assert_eq!(ts["channel"]["npm"], "https://npm.pkg.github.com");
    assert_eq!(ts["config_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(config["sdks"]["rust"]["version"], "0.2.0");

    let only: serde_json::Value = serde_json::from_str(&success(crucible(
        directory.path(),
        &["sdk", "config", "--json", "--sdk", "rust"],
    )))
    .unwrap();
    assert_eq!(
        only["sdks"].as_object().unwrap().keys().collect::<Vec<_>>(),
        ["rust"]
    );
    let text = success(crucible(directory.path(), &["sdk", "config"]));
    assert!(text.contains("[sdk.rust] Rust") && text.contains("[sdk.ts] TypeScript"));
    assert!(text.contains(ts["config_sha256"].as_str().unwrap()));

    let unknown = failure(crucible(
        directory.path(),
        &["sdk", "config", "--sdk", "go"],
    ));
    assert!(unknown.contains("declares no SDK \"go\""), "{unknown}");
    let unset = failure(
        Command::new(env!("CARGO_BIN_EXE_crucible"))
            .current_dir(directory.path())
            .env_remove("WEATHER_SDK_TEST_ENDPOINT")
            .args(["sdk", "config"])
            .output()
            .unwrap(),
    );
    assert!(
        unset.contains("WEATHER_SDK_TEST_ENDPOINT is not set"),
        "{unset}"
    );
}

#[test]
fn generate_writes_each_declared_sdk_to_its_out() {
    let directory = service(SDK_FILE);
    let root = directory.path();
    let config: serde_json::Value =
        serde_json::from_str(&success(crucible(root, &["sdk", "config", "--json"]))).unwrap();
    let stdout = success(crucible(root, &["sdk", "generate"]));
    assert!(stdout.contains("sdk.rust: wrote") && stdout.contains("sdk.ts: wrote"));

    let ts = root.join("generated/ts");
    let package = json(&ts.join("package.json"));
    assert_eq!(package["version"], "0.0.0-dev");
    assert_eq!(package["description"], "Client for the Weather API.");
    assert_eq!(package["license"], "MIT");
    assert_eq!(package["author"], "Weather <sdk@weather.example>");
    assert_eq!(
        package["publishConfig"],
        serde_json::json!({ "registry": "https://npm.pkg.github.com", "access": "restricted" })
    );
    assert_eq!(
        package["files"],
        serde_json::json!(["dist", "crucible.json", "crucible-model.json", "README.md"])
    );
    assert_eq!(
        fs::read_to_string(ts.join("README.md")).unwrap(),
        "# Weather\n\nThe SDK.\n"
    );
    assert!(
        fs::read_to_string(ts.join("src/client.ts"))
            .unwrap()
            .contains("https://api.weather.example")
    );
    let record = json(&ts.join("crucible.json"));
    assert_eq!(record["schemaVersion"], 2);
    assert_eq!(record["packageVersion"], "0.0.0-dev");
    assert_eq!(
        record["configSha256"],
        config["sdks"]["ts"]["config_sha256"]
    );
    assert!(record.get("source").is_none());
    assert!(ts.join("crucible-model.json").is_file());

    let rust = root.join("generated/rust");
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string(rust.join("Cargo.toml")).unwrap()).unwrap();
    let package = &manifest["package"];
    // Development exports take the CLI's version, never the floor.
    assert_eq!(package["version"].as_str(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(package["publish"].as_bool(), Some(false));
    assert_eq!(package["readme"].as_str(), Some("README.md"));
    assert_eq!(package["keywords"][0].as_str(), Some("weather"));
    assert_eq!(
        manifest["dependencies"]["crucible-client-runtime-rust"]["rev"].as_str(),
        Some("0123456789abcdef0123456789abcdef01234567")
    );
    let record = json(&rust.join("crucible.json"));
    assert_eq!(record["language"], "rust");
    assert_eq!(
        record["configSha256"],
        config["sdks"]["rust"]["config_sha256"]
    );

    // Regenerating keeps handwritten files and changes nothing generated.
    fs::write(rust.join("NOTES.md"), "mine").unwrap();
    let before = fs::read(rust.join("crucible.json")).unwrap();
    success(crucible(root, &["sdk", "generate", "--sdk", "rust"]));
    assert_eq!(fs::read(rust.join("crucible.json")).unwrap(), before);
    assert_eq!(fs::read_to_string(rust.join("NOTES.md")).unwrap(), "mine");
}

#[test]
fn an_invalid_sdk_file_reports_every_error_and_writes_nothing() {
    let directory = service(
        "schema_version = 1\nmodel = \"weather\"\n[sdk.rust]\nout = \"generated/rust\"\nendpoint = \"https://x.example\"\n[sdk.ts]\nout = \"generated/ts\"\n",
    );
    let error = failure(crucible(directory.path(), &["sdk", "generate"]));
    assert!(error.contains("sdk.rust.endpoint"), "{error}");
    assert!(
        error.contains("a TypeScript SDK needs `package`"),
        "{error}"
    );
    assert!(!directory.path().join("generated").exists());
}

#[test]
fn sdk_runs_only_at_the_revision_the_service_locks() {
    let directory = service(SDK_FILE);
    let root = directory.path();
    let lock = |revision: &str| {
        fs::write(
            root.join("Cargo.lock"),
            format!(
                "version = 4\n\n[[package]]\nname = \"crucible-build\"\nversion = \"0.6.1\"\nsource = \"git+https://github.com/Operation-Leyline/crucible?rev={revision}#{revision}\"\n"
            ),
        )
        .unwrap();
    };
    let other = "fedcba9876543210fedcba9876543210fedcba98";
    lock(other);
    let error = failure(crucible(root, &["sdk", "config"]));
    assert!(error.contains(other), "{error}");
    assert!(
        error.contains(
            "cargo install --locked --git https://github.com/Operation-Leyline/crucible --rev"
        ),
        "{error}"
    );
    assert!(!root.join("generated").exists());
    let error = failure(crucible(root, &["sdk", "generate"]));
    assert!(error.contains(other), "{error}");
    assert!(!root.join("generated").exists());
    if let Some(own) = crucible_revision::verified_revision() {
        lock(own);
        success(crucible(root, &["sdk", "config"]));
    }
    // A lock that takes Crucible from a path pins nothing.
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"crucible-build\"\nversion = \"0.6.1\"\n",
    )
    .unwrap();
    success(crucible(root, &["sdk", "config"]));
}

#[test]
fn a_missing_sdk_file_names_where_it_looked() {
    let directory = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    let error = failure(crucible(directory.path(), &["sdk", "config"]));
    assert!(error.contains("no crucible-sdk.toml"), "{error}");
    fs::write(directory.path().join("other.toml"), SDK_FILE).unwrap();
    common::install_shared(directory.path(), "weather");
    fs::create_dir(directory.path().join("sdk")).unwrap();
    fs::write(directory.path().join("sdk/README.md"), "# Weather\n").unwrap();
    success(crucible(
        directory.path(),
        &["sdk", "config", "--file", "other.toml"],
    ));
}

#[test]
fn generate_takes_the_common_keys_as_flags() {
    let directory = service(SDK_FILE);
    let root = directory.path();
    fs::write(root.join("LICENSE.txt"), "Proprietary.\r\n").unwrap();
    let common = [
        "--description",
        "Weather, ad hoc.",
        "--repository",
        "https://github.com/example/weather",
        "--homepage",
        "https://weather.example",
        "--author",
        "A <a@weather.example>",
        "--author",
        "B <b@weather.example>",
        "--keyword",
        "weather",
        "--readme",
        "sdk/README.md",
        "--license-file",
        "LICENSE.txt",
        "--private",
    ];
    let mut ts = vec![
        "generate",
        "ts",
        "weather",
        "--out",
        "adhoc/ts",
        "--package-name",
        "@example/adhoc",
    ];
    ts.extend(common);
    success(crucible(root, &ts));
    let package = json(&root.join("adhoc/ts/package.json"));
    assert_eq!(package["description"], "Weather, ad hoc.");
    assert_eq!(package["homepage"], "https://weather.example");
    assert_eq!(package["contributors"][0], "B <b@weather.example>");
    assert_eq!(package["license"], "SEE LICENSE IN LICENSE");
    assert_eq!(package["private"], true);
    assert_eq!(
        fs::read_to_string(root.join("adhoc/ts/LICENSE")).unwrap(),
        "Proprietary.\n"
    );
    let record = json(&root.join("adhoc/ts/crucible.json"));
    assert!(record.get("configSha256").is_none());

    let mut rust = vec![
        "generate",
        "rust",
        "weather",
        "--client",
        "--out",
        "adhoc/rust",
        "--package-name",
        "adhoc-sdk",
    ];
    rust.extend(common);
    success(crucible(root, &rust));
    let manifest = fs::read_to_string(root.join("adhoc/rust/Cargo.toml")).unwrap();
    assert!(
        manifest.contains("description = \"Weather, ad hoc.\""),
        "{manifest}"
    );
    assert!(
        manifest.contains("license-file = \"LICENSE\""),
        "{manifest}"
    );
    assert!(manifest.contains("publish = false"), "{manifest}");
    assert!(root.join("adhoc/rust/crucible.json").is_file());

    let error = failure(crucible(
        root,
        &[
            "generate",
            "rust",
            "weather",
            "--out",
            "server.rs",
            "--private",
        ],
    ));
    assert!(error.contains("--client"), "{error}");
    let error = failure(crucible(
        root,
        &[
            "generate",
            "rust",
            "weather",
            "--client",
            "--out",
            "adhoc/bad",
            "--keyword",
            "9lives",
        ],
    ));
    assert!(error.contains("9lives"), "{error}");
}

/// Selecting an SDK reads only its environment; without a Cargo.lock the
/// revision is not checked, and the command says so.
#[test]
fn selecting_one_sdk_needs_only_its_environment() {
    let directory = service(SDK_FILE);
    let output = Command::new(env!("CARGO_BIN_EXE_crucible"))
        .current_dir(directory.path())
        .env_remove("WEATHER_SDK_TEST_ENDPOINT")
        .args(["sdk", "config", "--sdk", "rust"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    success(output);
    assert!(stderr.contains("no Cargo.lock"), "{stderr}");
    // Every SDK is still checked, selected or not.
    let broken = SDK_FILE.replace("[sdk.ts]", "[sdk.ts]\nruntime_path = \"x\"");
    fs::write(directory.path().join("crucible-sdk.toml"), broken).unwrap();
    let error = failure(crucible(
        directory.path(),
        &["sdk", "config", "--sdk", "rust"],
    ));
    assert!(error.contains("sdk.ts.runtime_path"), "{error}");
}

#[test]
fn generate_ts_takes_an_npm_registry() {
    let directory = service(SDK_FILE);
    let root = directory.path();
    success(crucible(
        root,
        &[
            "generate",
            "ts",
            "weather",
            "--out",
            "adhoc/ts",
            "--package-name",
            "@example/adhoc",
            "--npm-registry",
            "https://npm.pkg.github.com",
            "--npm-access",
            "restricted",
            "--private",
        ],
    ));
    let package = json(&root.join("adhoc/ts/package.json"));
    assert_eq!(
        package["publishConfig"],
        serde_json::json!({ "registry": "https://npm.pkg.github.com", "access": "restricted" })
    );
    assert!(package.get("private").is_none());
    let error = failure(crucible(
        root,
        &[
            "generate",
            "ts",
            "weather",
            "--out",
            "adhoc/public",
            "--package-name",
            "@example/adhoc",
            "--npm-registry",
            "https://%72egistry.npmjs.org",
            "--private",
        ],
    ));
    assert!(error.contains("public registry"), "{error}");
}

#[test]
fn the_version_names_the_revision_and_config_shows_the_whole_channel() {
    let directory = service(SDK_FILE);
    let version = success(crucible(directory.path(), &["--version"]));
    assert!(version.contains(env!("CARGO_PKG_VERSION")), "{version}");
    if let Some(revision) = crucible_revision::recorded_revision() {
        assert!(version.contains(&format!("({revision})")), "{version}");
    }
    let text = success(crucible(directory.path(), &["sdk", "config"]));
    for row in [
        "channel.git",
        "channel.branch",
        "channel.path",
        "channel.tag",
        "channel.adopt",
        "channel.npm",
        "channel.access",
        "channel.dist_tag",
    ] {
        assert!(text.contains(row), "{row} missing from:\n{text}");
    }
}

/// The adversarial inputs that once wrote outside the service: refused,
/// and nothing is written anywhere.
#[test]
fn a_drive_prefixed_out_writes_nothing() {
    let directory = service(SDK_FILE);
    let root = directory.path();
    for out in ["gen/C:escaped2", "gen/D:pwned", "gen."] {
        fs::write(
            root.join("crucible-sdk.toml"),
            SDK_FILE.replace("generated/rust", out),
        )
        .unwrap();
        let error = failure(crucible(root, &["sdk", "generate", "--sdk", "rust"]));
        assert!(error.contains("sdk.rust.out"), "{error}");
        assert!(!root.join("escaped2").exists() && !root.join("gen").exists());
    }
}

/// npm has no documentation field: the flag is accepted and not recorded.
#[test]
fn generate_ts_accepts_documentation_without_a_field_for_it() {
    let directory = service(SDK_FILE);
    let root = directory.path();
    success(crucible(
        root,
        &[
            "generate",
            "ts",
            "weather",
            "--out",
            "adhoc/docs",
            "--package-name",
            "@example/adhoc",
            "--documentation",
            "https://docs.weather.example",
        ],
    ));
    let text = fs::read_to_string(root.join("adhoc/docs/package.json")).unwrap();
    assert!(!text.contains("docs.weather.example"), "{text}");
}

#[test]
fn describe_prints_each_sdks_surface_as_its_generator_names_it() {
    let directory = service(SDK_FILE);
    let root = directory.path();
    let document: serde_json::Value =
        serde_json::from_str(&success(crucible(root, &["sdk", "describe"]))).unwrap();
    assert_eq!(document["format"], "crucible.sdks/1");
    assert_eq!(document["service"], "example.weather#Weather");
    let ts = &document["sdks"]["ts"];
    assert_eq!(ts["package"]["name"], "@example/weather");
    assert_eq!(ts["client"]["name"], "WeatherClient");
    // The environment holds a deployment's endpoint, which a description
    // never reads; a released SDK has it generated in.
    assert_eq!(ts["endpoint"], serde_json::Value::Null);
    assert_eq!(ts["endpointEnv"], "WEATHER_SDK_TEST_ENDPOINT");
    assert_eq!(ts["client"]["construct"], "new WeatherClient()");
    assert_eq!(
        ts["operations"]["example.weather#ListCities"]["paginators"][0],
        "paginateListCities"
    );
    let rust = &document["sdks"]["rust"];
    assert_eq!(
        rust["operations"]["example.weather#GetCity"]["method"],
        "get_city"
    );

    // An unset endpoint variable is not needed, the output is the same
    // bytes, and --out writes only the selected SDKs, creating its
    // directory.
    let output = Command::new(env!("CARGO_BIN_EXE_crucible"))
        .current_dir(root)
        .env_remove("WEATHER_SDK_TEST_ENDPOINT")
        .args([
            "sdk",
            "describe",
            "--sdk",
            "ts",
            "--out",
            "target/docs/sdks.json",
        ])
        .output()
        .unwrap();
    success(output);
    let written = json(&root.join("target/docs/sdks.json"));
    assert_eq!(written["sdks"]["ts"], document["sdks"]["ts"]);
    assert_eq!(
        written["sdks"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["ts"]
    );
    assert_eq!(written["sdks"]["ts"]["endpoint"], serde_json::Value::Null);
    assert_eq!(
        written["sdks"]["ts"]["endpointEnv"],
        "WEATHER_SDK_TEST_ENDPOINT"
    );
    let error = failure(crucible(root, &["sdk", "describe", "--sdk", "go"]));
    assert!(error.contains("declares no SDK \"go\""), "{error}");
}

#[test]
fn a_reference_refuses_an_sdk_file_for_another_model() {
    let directory = service(SDK_FILE);
    let root = directory.path();
    common::install_shared(root, "client-docs");
    let error = failure(crucible(
        root,
        &[
            "generate",
            "docs",
            "client-docs",
            "--out",
            "reference",
            "--sdks",
            "crucible-sdk.toml",
        ],
    ));
    assert!(error.contains("declares SDKs for the model"), "{error}");
}
