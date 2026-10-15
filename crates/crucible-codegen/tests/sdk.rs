//! `crucible-sdk.toml` and its one resolver (docs/sdk-packaging.md,
//! "Declaring SDKs").

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crucible_codegen::sdk::{self, Channel, Endpoint, Language};

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

/// A service repository holding `text` as its SDK file, a model and a README.
fn repository(text: &str) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    fs::write(directory.path().join("model.json"), WEATHER).unwrap();
    fs::create_dir_all(directory.path().join("sdk/rust")).unwrap();
    fs::write(directory.path().join("sdk/README.md"), "# Weather\n").unwrap();
    fs::write(directory.path().join("sdk/rust/README.md"), "# Rust\n").unwrap();
    fs::write(directory.path().join("LICENSE"), "Proprietary.\n").unwrap();
    let path = directory.path().join(sdk::FILE_NAME);
    fs::write(&path, text).unwrap();
    (directory, path)
}

fn errors(text: &str) -> Vec<String> {
    let (_directory, path) = repository(text);
    match sdk::load(&path) {
        Ok(_) => Vec::new(),
        Err(errors) => errors.0,
    }
}

/// Every error names its key.
fn assert_error(errors: &[String], key: &str, message: &str) {
    assert!(
        errors
            .iter()
            .any(|error| error.contains(&format!(": {key}: ")) && error.contains(message)),
        "no error at {key} saying {message:?} in {errors:#?}"
    );
}

const PIGEON: &str = r#"
schema_version = 1
model = "model.json"

[defaults]
description = "Client for the Weather API."
repository = "https://github.com/example/weather"
homepage = "https://weather.example"
license = "LicenseRef-Proprietary"
authors = ["Weather <sdk@weather.example>"]
readme = "sdk/README.md"
keywords = ["weather", "forecast"]
private = true

[sdk.ts]
package = "@example/weather"
endpoint = "https://api.weather.example"
out = "generated/weather-ts"
channel = { npm = "https://npm.pkg.github.com", access = "restricted" }

[sdk.rust]
package = "weather-sdk"
version = "0.2.0"
repository = "https://github.com/example/weather-rs"
out = "generated/weather-rust"
readme = "sdk/rust/README.md"
license_file = "LICENSE"
keywords = []
test = [["cargo", "test", "--locked", "--manifest-path", "sdk/rust-tests/Cargo.toml"]]
lockfiles = ["sdk/rust-tests/Cargo.lock"]
channel = { git = "git@github.com:example/weather-rs.git", tag = "v{version}" }

[sdk.java-internal]
language = "java"
audience = "internal"
package = "example.weather.internal"
group_id = "example.weather"
artifact_id = "weather-internal"
private = false
out = "generated/weather-java-internal"
"#;

#[test]
fn defaults_apply_key_by_key_and_an_sdk_overrides_them() {
    let (_directory, path) = repository(PIGEON);
    let file = sdk::load(&path).unwrap();
    assert_eq!(file.model, "model.json");
    assert_eq!(
        file.sdks.keys().collect::<Vec<_>>(),
        ["java-internal", "rust", "ts"]
    );
    let ts = &file.sdks["ts"];
    assert_eq!(ts.language, Language::Ts);
    assert_eq!(
        ts.metadata.repository.as_deref(),
        Some("https://github.com/example/weather")
    );
    assert_eq!(ts.metadata.readme.as_deref(), Some("sdk/README.md"));
    assert_eq!(
        ts.metadata.license.as_deref(),
        Some("LicenseRef-Proprietary")
    );
    assert_eq!(ts.metadata.keywords, ["weather", "forecast"]);
    assert!(ts.metadata.private);
    assert_eq!(
        ts.options.endpoint,
        Some(Endpoint::Url("https://api.weather.example".into()))
    );
    assert!(matches!(
        &ts.channel,
        Some(Channel::Npm { dist_tag, access, .. }) if dist_tag == "latest" && access.as_deref() == Some("restricted")
    ));

    let rust = &file.sdks["rust"];
    assert_eq!(rust.language, Language::Rust);
    assert_eq!(rust.version.as_deref(), Some("0.2.0"));
    // The SDK's own keys win; a list it sets replaces the default's.
    assert_eq!(
        rust.metadata.repository.as_deref(),
        Some("https://github.com/example/weather-rs")
    );
    assert_eq!(rust.metadata.readme.as_deref(), Some("sdk/rust/README.md"));
    assert!(rust.metadata.keywords.is_empty());
    // `license` and `license_file` are one setting: the SDK's file replaces
    // the default expression.
    assert_eq!(rust.metadata.license, None);
    assert_eq!(rust.metadata.license_file.as_deref(), Some("LICENSE"));
    assert_eq!(rust.test.len(), 1);
    assert!(matches!(
        &rust.channel,
        Some(Channel::Git { branch, path, tag, .. }) if branch == "main" && path == "." && tag == "v{version}"
    ));

    let java = &file.sdks["java-internal"];
    assert_eq!(java.language, Language::Java);
    assert_eq!(java.audience, crucible_codegen::Audience::Internal);
    assert!(!java.metadata.private);
    assert!(java.channel.is_none());
}

#[test]
fn every_error_is_reported_at_its_key_in_one_run() {
    let errors = errors(
        r#"
schema_version = 1
model = "C:/model.json"
colour = "blue"

[defaults]
out = "x"
endpoint = "https://x.example"
licence = "MIT"
repository = "github.com/example"

[sdk.rust]
endpoint = "https://api.example"
out = "out\\rust"
keywords = ["9lives", "rust"]
provenance = "minimal"
channel = { npm = "https://npm.pkg.github.com" }

[sdk.ts]
language = "java"
out = "out/ts"

[sdk.java-internal]
out = "out/java"

[sdk.python]
package = "weather"
out = "out/python"
version = "1.0"
private = true

[sdk.Bad]
out = "x"
"#,
    );
    assert_error(&errors, "model", "is absolute");
    assert_error(&errors, "colour", "unknown key");
    assert_error(&errors, "defaults.out", "only the common keys");
    assert_error(&errors, "defaults.endpoint", "only the common keys");
    assert_error(&errors, "defaults.licence", "unknown key");
    assert_error(&errors, "defaults.repository", "not an http or https URL");
    assert_error(
        &errors,
        "sdk.rust.endpoint",
        "the Rust generator takes no `endpoint`",
    );
    assert_error(&errors, "sdk.rust.out", "write paths with `/`");
    assert_error(&errors, "sdk.rust.keywords", "\"9lives\"");
    assert_error(&errors, "sdk.rust.provenance", "not built yet");
    assert_error(
        &errors,
        "sdk.rust.channel.npm",
        "publishes TypeScript SDKs, not Rust",
    );
    assert_error(&errors, "sdk.ts.language", "is the TypeScript SDK");
    assert_error(&errors, "sdk.java-internal", "set `language`");
    assert_error(&errors, "sdk.python", "needs `module_name`");
    assert_error(&errors, "sdk.python.version", "not a version floor");
    assert_error(
        &errors,
        "sdk.python.channel",
        "a private SDK names its channel",
    );
    assert_error(&errors, "sdk.Bad", "lowercase letter");
}

#[test]
fn schema_versions_and_required_keys() {
    assert_error(&errors("model = \"m\"\n"), "schema_version", "missing");
    assert_error(
        &errors("schema_version = 2\nmodel = \"m\"\n"),
        "schema_version",
        "not a schema this crucible reads",
    );
    assert_error(&errors("schema_version = 1\n"), "model", "missing");
    assert_error(
        &errors("schema_version = 1\nmodel = \"m\"\n[sdk.rust]\n"),
        "sdk.rust",
        "set `out`",
    );
    assert_error(
        &errors("schema_version = 1\nmodel = \"m\"\nservice = \"nohash\"\n"),
        "service",
        "not a shape ID",
    );
    assert!(errors("schema_version = 1\nmodel = \"m\"\n").is_empty());
    let errors = errors("not toml =");
    assert_eq!(errors.len(), 1);
}

#[test]
fn channels_never_carry_credentials_or_reach_public_registries_by_mistake() {
    let base = "schema_version = 1\nmodel = \"m\"\n";
    let ts = |extra: &str| format!("{base}[sdk.ts]\npackage = \"@x/y\"\nout = \"out\"\n{extra}\n");
    assert_error(
        &errors(&ts(
            "channel = { npm = \"https://user:token@npm.pkg.github.com\" }",
        )),
        "sdk.ts.channel.npm",
        "never carries credentials",
    );
    assert_error(
        &errors(&ts(
            "channel = { npm = \"https://ghp_abc@npm.pkg.github.com\" }",
        )),
        "sdk.ts.channel.npm",
        "never carries credentials",
    );
    assert_error(
        &errors(&ts(
            "channel = { git = \"https://x:y@github.com/a/b.git\" }",
        )),
        "sdk.ts.channel.git",
        "password or token",
    );
    assert_error(
        &errors(&ts("channel = { git = \"user:pw@github.com:a/b.git\" }")),
        "sdk.ts.channel.git",
        "password or token",
    );
    // A user name is fine over SSH.
    assert!(errors(&ts("channel = { git = \"git@github.com:a/b.git\" }")).is_empty());
    assert!(errors(&ts("channel = { git = \"ssh://git@github.com/a/b.git\" }")).is_empty());
    assert_error(
        &errors(&ts(
            "private = true\nchannel = { npm = \"https://registry.npmjs.org/\" }",
        )),
        "sdk.ts.channel",
        "never goes to a public registry",
    );
    assert_error(
        &errors(&ts(
            "audience = \"internal\"\nchannel = { npm = \"https://registry.npmjs.org\" }",
        )),
        "sdk.ts.channel",
        "internal-audience SDK",
    );
    assert!(errors(&ts("channel = { npm = \"https://registry.npmjs.org\" }")).is_empty());
    assert_error(
        &errors(&ts("channel = { git = \"git@x:y\", tag = \"release\" }")),
        "sdk.ts.channel.tag",
        "{version}",
    );
    assert_error(
        &errors(&ts("channel = { git = \"git@x:y\", npm = \"https://x\" }")),
        "sdk.ts.channel",
        "exactly one",
    );
    assert_error(
        &errors(&ts("channel = { cargo = \"crates-io\" }")),
        "sdk.ts.channel.cargo",
        "not built yet",
    );
    assert_error(
        &errors(&ts(
            "channel = { git = \"git@x:y\", tags = \"v{version}\" }",
        )),
        "sdk.ts.channel.tags",
        "takes no `tags`",
    );
}

#[test]
fn language_keys_are_typed() {
    let base = "schema_version = 1\nmodel = \"m\"\n";
    assert_error(
        &errors(&format!(
            "{base}[sdk.ts]\npackage = \"x\"\nout = \"o\"\npackage_json = {{ name = \"other\", bugs = \"https://x\" }}\nmiddleware = [\"gzip\"]\n"
        )),
        "sdk.ts.package_json",
        "cannot set `name`",
    );
    assert_error(
        &errors(&format!(
            "{base}[sdk.ts]\npackage = \"x\"\nout = \"o\"\nmiddleware = [\"gzip\"]\nendpoint = {{ env = \"1BAD\" }}\n"
        )),
        "sdk.ts.endpoint",
        "is not an environment variable name",
    );
    assert_error(
        &errors(&format!(
            "{base}[sdk.rust]\nout = \"o\"\nruntime = {{ git = \"https://x\" }}\nruntime_path = \"../crucible\"\n"
        )),
        "sdk.rust.runtime",
        "needs `rev`",
    );
    assert_error(
        &errors(&format!(
            "{base}[sdk.rust]\nout = \"o\"\nruntime = {{ git = \"https://x\", rev = \"r\", version = \"0.6\" }}\nruntime_path = \"../crucible\"\n"
        )),
        "sdk.rust.runtime_path",
        "not both",
    );
    assert_error(
        &errors(&format!(
            "{base}[sdk.a]\nlanguage = \"rust\"\nout = \"o\"\n[sdk.b]\nlanguage = \"rust\"\nout = \"o/\"\n"
        )),
        "sdk.b.out",
        "each SDK needs its own",
    );
}

#[test]
fn endpoints_from_the_environment_are_read_by_resolve_not_load() {
    let text = "schema_version = 1\nmodel = \"model.json\"\n[sdk.ts]\npackage = \"x\"\nout = \"o\"\nendpoint = { env = \"WEATHER_SDK_ENDPOINT\" }\n";
    let (_directory, path) = repository(text);
    let loaded = sdk::load(&path).unwrap();
    assert_eq!(
        loaded.sdks["ts"].options.endpoint,
        Some(Endpoint::Env {
            name: "WEATHER_SDK_ENDPOINT".into(),
            value: None
        })
    );
    let error = sdk::resolve_with(&path, |_| None).unwrap_err().to_string();
    assert!(error.contains("sdk.ts.endpoint") && error.contains("WEATHER_SDK_ENDPOINT"));
    let resolved = sdk::resolve_with(&path, |name| {
        (name == "WEATHER_SDK_ENDPOINT").then(|| "https://api.example".into())
    })
    .unwrap();
    let ts = &resolved.sdks["ts"];
    assert_eq!(
        ts.options.endpoint.as_ref().unwrap().value(),
        Some("https://api.example")
    );
    let effective = ts.effective(&resolved);
    assert_eq!(effective["endpoint"], "https://api.example");
    assert_eq!(effective["endpoint_env"], "WEATHER_SDK_ENDPOINT");
}

/// The configuration digest depends on what the file means, not how it is
/// written, and moves with any value.
#[test]
fn the_configuration_digest_is_canonical() {
    let a = "schema_version = 1\nmodel = \"model.json\"\n[defaults]\nlicense = \"MIT\"\n[sdk.ts]\npackage = \"x\"\nout = \"o\"\npackage_json = { bugs = \"https://b\", author = \"a\" }\n";
    let b = "model = \"model.json\"\nschema_version = 1\n[sdk.ts]\npackage_json = { author = \"a\", bugs = \"https://b\" }\nout = \"o\"\nlicense = \"MIT\"\npackage = \"x\"\n";
    let digest = |text: &str| {
        let (_directory, path) = repository(text);
        let file = sdk::load(&path).unwrap();
        file.sdks["ts"].config_sha256(&file)
    };
    assert_eq!(digest(a), digest(b));
    assert_ne!(digest(a), digest(&a.replace("MIT", "Apache-2.0")));
    assert_eq!(digest(a).len(), 64);
}

#[test]
fn the_effective_configuration_is_json_with_digests() {
    let (_directory, path) = repository(PIGEON);
    let file = sdk::load(&path).unwrap();
    let json = file.to_json();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["model"], "model.json");
    let rust = &json["sdks"]["rust"];
    assert_eq!(rust["language"], "rust");
    assert_eq!(rust["package"], "weather-sdk");
    assert_eq!(rust["license_file"], "LICENSE");
    assert_eq!(
        rust["channel"]["git"],
        "git@github.com:example/weather-rs.git"
    );
    assert_eq!(rust["test"][0][0], "cargo");
    assert_eq!(
        rust["config_sha256"],
        file.sdks["rust"].config_sha256(&file)
    );
    assert_eq!(json["sdks"]["java-internal"]["audience"], "internal");
}

#[test]
fn the_file_is_found_up_to_the_repository_root() {
    let (directory, path) = repository("schema_version = 1\nmodel = \"m\"\n");
    let nested = directory.path().join("a/b");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(sdk::find(&nested), Some(path.clone()));
    // A nested repository stops the search at its root.
    fs::create_dir(nested.join(".git")).unwrap();
    assert_eq!(sdk::find(&nested), None);
}

#[test]
fn select_names_unknown_sdks() {
    let (_directory, path) = repository(PIGEON);
    let file = sdk::load(&path).unwrap();
    assert_eq!(file.select(&[]).unwrap().len(), 3);
    assert_eq!(file.select(&["rust".into()]).unwrap()[0].name, "rust");
    let error = file.select(&["go".into()]).unwrap_err();
    assert!(
        error.contains("\"go\"") && error.contains("\"rust\""),
        "{error}"
    );
}

#[cfg(all(feature = "rust", feature = "ts"))]
#[test]
fn a_declared_sdk_generates_at_its_development_version_with_its_metadata() {
    let (directory, path) = repository(PIGEON);
    let file = sdk::load(&path).unwrap();
    let service = crucible_smithy::Model::from_json(WEATHER)
        .unwrap()
        .lower(None)
        .unwrap();
    let ts = &file.sdks["ts"];
    let crucible_codegen::Client::Ts(options) = ts.client(&file, None).unwrap() else {
        panic!("a TypeScript client");
    };
    assert_eq!(options.package_version, "0.0.0-dev");
    assert_eq!(
        options.endpoint.as_deref(),
        Some("https://api.weather.example")
    );
    assert_eq!(options.metadata.readme.as_deref(), Some("# Weather\n"));
    assert_eq!(
        options.metadata.registry,
        Some(crucible_codegen::Registry::npm(
            "https://npm.pkg.github.com",
            Some("restricted")
        ))
    );
    let crucible_codegen::Client::Rust(rust) =
        file.sdks["rust"].client(&file, Some("0.2.1")).unwrap()
    else {
        panic!("a Rust client");
    };
    assert_eq!(rust.package_version, "0.2.1");
    assert_eq!(
        rust.metadata.license_file.as_deref(),
        Some("Proprietary.\n")
    );
    assert_eq!(rust.metadata.readme.as_deref(), Some("# Rust\n"));
    assert!(rust.metadata.private);

    // A README checked out with CRLF digests as the LF one: copied text is LF.
    let digest = || {
        let file = sdk::load(&path).unwrap();
        let client = file.sdks["ts"].client(&file, None).unwrap();
        let files = client.emit(&service).unwrap();
        let record: serde_json::Value = serde_json::from_str(
            &files
                .iter()
                .find(|file| file.path == "crucible.json")
                .unwrap()
                .contents,
        )
        .unwrap();
        let readme = files.iter().find(|file| file.path == "README.md").unwrap();
        assert!(!readme.contents.contains('\r'));
        record["contentDigest"].clone()
    };
    let lf = digest();
    fs::write(directory.path().join("sdk/README.md"), "# Weather\r\n").unwrap();
    assert_eq!(digest(), lf);
    fs::write(directory.path().join("sdk/README.md"), "# Weather!\n").unwrap();
    assert_ne!(digest(), lf);
}

#[test]
fn development_versions_follow_each_ecosystem() {
    let version = |language: Language| language.development_version();
    assert_eq!(version(Language::Ts).as_deref(), Some("0.0.0-dev"));
    assert_eq!(version(Language::Python).as_deref(), Some("0.0.0.dev0"));
    assert_eq!(version(Language::Kotlin).as_deref(), Some("0.0.0-SNAPSHOT"));
    assert_eq!(version(Language::R).as_deref(), Some("0.0.0"));
    assert_eq!(
        version(Language::Rust).as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(version(Language::Go), None);
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A release's `source` reproduces its inputs, or says it does not.
#[test]
fn the_source_commit_must_reproduce_every_input() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "SDK source test"]);
    git(root, &["config", "user.email", "sdk@example.invalid"]);
    git(
        root,
        &[
            "remote",
            "add",
            "origin",
            "https://token@github.com/example/weather.git",
        ],
    );
    fs::write(root.join("model.json"), WEATHER).unwrap();
    fs::create_dir(root.join("sdk")).unwrap();
    fs::write(root.join("sdk/README.md"), "# Weather\n").unwrap();
    fs::write(
        root.join(sdk::FILE_NAME),
        "schema_version = 1\nmodel = \"model.json\"\n[sdk.rust]\nout = \"generated/rust\"\nreadme = \"sdk/README.md\"\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "service"]);
    let file = sdk::load(&root.join(sdk::FILE_NAME)).unwrap();
    let rust = &file.sdks["rust"];
    let source = sdk::source(&file, rust, false).unwrap();
    assert_eq!(source.commit.len(), 40);
    assert!(!source.dirty);
    assert_eq!(
        source.repository.as_deref(),
        Some("https://github.com/example/weather")
    );
    // Generated output is not an input.
    fs::create_dir_all(root.join("generated/rust")).unwrap();
    fs::write(root.join("generated/rust/Cargo.toml"), "x").unwrap();
    assert!(sdk::source(&file, rust, false).is_ok());
    fs::write(root.join("sdk/README.md"), "# Changed\n").unwrap();
    let error = sdk::source(&file, rust, false).unwrap_err();
    assert!(
        error.contains("sdk/README.md") && error.contains("--allow-dirty"),
        "{error}"
    );
    assert!(sdk::source(&file, rust, true).unwrap().dirty);
    git(root, &["checkout", "--", "sdk/README.md"]);
    fs::write(root.join("model.json"), WEATHER.replace("2026", "2027")).unwrap();
    assert!(sdk::source(&file, rust, false).is_err());
    git(root, &["checkout", "--", "model.json"]);
    fs::remove_file(root.join("sdk/README.md")).unwrap();
    git(root, &["rm", "-q", "--cached", "sdk/README.md"]);
    fs::write(root.join("sdk/README.md"), "# Weather\n").unwrap();
    let error = sdk::source(&file, rust, false).unwrap_err();
    // Each input is named once, with one reason.
    assert_eq!(error.matches("sdk/README.md").count(), 1, "{error}");
    assert!(error.contains("sdk/README.md is not tracked"), "{error}");
}

const SENTINEL: &str = "sentinel-secret-5d81";

fn sdk_errors(sdks: &str) -> Vec<String> {
    errors(&format!("schema_version = 1\nmodel = \"m\"\n{sdks}"))
}

/// No path leaves the SDK file's directory: nothing outside the service is
/// written, copied into a package, or adopted by a channel.
#[test]
fn paths_stay_inside_the_sdk_files_directory() {
    let errors = errors(
        r#"
schema_version = 1
model = "../model.json"
[sdk.rust]
out = "../escaped"
readme = "../secret.md"
license_file = "./LICENSE"
lockfiles = ["a/../b"]
channel = { git = "git@x:y", path = "../..", adopt = ["../../..", "docs"] }
"#,
    );
    for key in [
        "model",
        "sdk.rust.out",
        "sdk.rust.readme",
        "sdk.rust.license_file",
        "sdk.rust.lockfiles",
        "sdk.rust.channel.path",
        "sdk.rust.channel.adopt",
    ] {
        assert_error(&errors, key, "leaves the SDK file's directory");
    }
    // `.` is the distribution repository's root, and a trailing `/` is fine.
    let fine = sdk_errors(
        "[sdk.rust]\nout = \"generated/rust/\"\nchannel = { git = \"git@x:y\", path = \".\" }\n",
    );
    assert!(fine.is_empty(), "{fine:#?}");
}

/// A README or license that resolves outside the SDK file's directory,
/// through a symlink, is never copied into a package.
#[test]
fn a_readme_symlinked_outside_the_service_is_refused() {
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.md"), SENTINEL).unwrap();
    let (directory, path) = repository(
        "schema_version = 1\nmodel = \"model.json\"\n[sdk.rust]\nout = \"o\"\nreadme = \"sdk/linked.md\"\n",
    );
    let link = directory.path().join("sdk/linked.md");
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(outside.path().join("secret.md"), &link);
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(outside.path().join("secret.md"), &link);
    if linked.is_err() {
        // Creating symlinks needs a privilege this machine does not grant.
        return;
    }
    let file = sdk::load(&path).unwrap();
    let error = file.sdks["rust"].package_metadata(&file).unwrap_err();
    assert!(error.contains("resolves outside"), "{error}");
    assert!(!error.contains(SENTINEL));
}

/// Two SDKs that would write one directory, or one inside the other, are
/// refused before anything is written, whatever the spelling's case.
#[test]
fn equivalent_or_nested_outs_are_refused() {
    let rust = |out: &str, other: &str| {
        sdk_errors(&format!(
            "[sdk.a]\nlanguage = \"rust\"\nout = \"{out}\"\n[sdk.b]\nlanguage = \"rust\"\nout = \"{other}\"\n"
        ))
    };
    for (out, other) in [
        ("gen", "gen/"),
        ("gen", "GEN"),
        ("gen", "gen/rust"),
        ("Gen/Rust", "gen"),
    ] {
        assert_error(&rust(out, other), "sdk.b.out", "overlaps sdk.a's out");
    }
    assert!(rust("gen/rust", "gen/ts").is_empty());
    assert!(rust("gen", "generated").is_empty());
}

/// Published URLs (repository, homepage, endpoint) and channel and runtime
/// URLs never carry credentials, and no message repeats one.
#[test]
fn credentials_are_refused_everywhere_and_never_repeated() {
    let text = [
        "[defaults]",
        "repository = \"https://u:SENTINEL@github.com/x\"",
        "homepage = \"https://SENTINEL@weather.example\"",
        "[sdk.ts]",
        "package = \"x\"",
        "out = \"ts\"",
        "endpoint = \"https://u:SENTINEL@api.example\"",
        "channel = { npm = \"https://u:SENTINEL@npm.pkg.github.com\", access = \"everyone\" }",
        "[sdk.rust]",
        "out = \"rust\"",
        "runtime = { git = \"https://t:SENTINEL@github.com/x\", rev = \"r\", version = \"0.6\" }",
        "channel = { git = \"u:SENTINEL@github.com:x.git\", tag = \"release\" }",
    ]
    .join("\n")
    .replace("SENTINEL", SENTINEL);
    let errors = sdk_errors(&text);
    for key in [
        "defaults.repository",
        "defaults.homepage",
        "sdk.ts.endpoint",
        "sdk.ts.channel.npm",
        "sdk.rust.runtime.git",
        "sdk.rust.channel.git",
    ] {
        assert!(
            errors
                .iter()
                .any(|error| error.contains(&format!(": {key}: "))),
            "no error at {key} in {errors:#?}"
        );
    }
    // A credential does not stop the rest of the channel being checked.
    assert_error(
        &errors,
        "sdk.ts.channel.access",
        "not \"public\" or \"restricted\"",
    );
    assert_error(&errors, "sdk.rust.channel.tag", "{version}");
    for error in &errors {
        assert!(!error.contains(SENTINEL), "{error}");
    }
}

/// The private-registry guard reads the host as the network does.
#[test]
fn every_spelling_of_the_public_registry_is_public() {
    for url in [
        "https://%72egistry.npmjs.org",
        "https://registry.npmjs.org.",
        "https://REGISTRY.NPMJS.ORG/",
    ] {
        assert_error(
            &sdk_errors(&format!(
                "[sdk.ts]\npackage = \"x\"\nout = \"o\"\nprivate = true\nchannel = {{ npm = \"{url}\" }}\n"
            )),
            "sdk.ts.channel",
            "never goes to a public registry",
        );
    }
}

/// Everything wrong is reported in one run: an invalid SDK name's table,
/// a channel of an unknown language, every bad list element.
#[test]
fn one_run_reports_errors_that_hide_behind_others() {
    let text = [
        "[sdk.Bad]",
        "language = \"rust\"",
        "out = \"bad\"",
        "endpoint = \"https://x.example\"",
        "[sdk.weird]",
        "language = \"cobol\"",
        "out = \"weird\"",
        "channel = { git = \"https://u:pw@github.com/x\" }",
        "[sdk.ts]",
        "package = \"x\"",
        "out = \"ts\"",
        "keywords = [1, \"fine\", 2]",
    ]
    .join("\n");
    let errors = sdk_errors(&text);
    assert_error(&errors, "sdk.Bad", "lowercase letter");
    assert_error(&errors, "sdk.Bad.endpoint", "takes no `endpoint`");
    assert_error(&errors, "sdk.weird.language", "not a client language");
    assert_error(&errors, "sdk.weird.channel.git", "password or token");
    assert_eq!(
        errors
            .iter()
            .filter(|error| error.contains(": sdk.ts.keywords: must be a string"))
            .count(),
        2,
        "{errors:#?}"
    );
}

/// Selecting SDKs reads only their environment, and a variable's value is
/// checked like a written endpoint.
#[test]
fn endpoints_are_read_only_for_the_selected_sdks_and_checked() {
    let text = "schema_version = 1\nmodel = \"model.json\"\n[sdk.ts]\npackage = \"x\"\nout = \"ts\"\nendpoint = { env = \"WEATHER_TS_ENDPOINT\" }\n[sdk.python]\npackage = \"x\"\nmodule_name = \"x\"\nout = \"py\"\nendpoint = { env = \"WEATHER_PY_ENDPOINT\" }\n";
    let (_directory, path) = repository(text);
    let mut file = sdk::load(&path).unwrap();
    file.resolve_endpoints(&["ts".into()], |name| {
        (name == "WEATHER_TS_ENDPOINT").then(|| "https://ts.example".into())
    })
    .unwrap();
    assert_eq!(
        file.sdks["ts"].options.endpoint.as_ref().unwrap().value(),
        Some("https://ts.example")
    );
    assert_eq!(
        file.sdks["python"]
            .options
            .endpoint
            .as_ref()
            .unwrap()
            .value(),
        None
    );
    for value in [
        "not a url".to_owned(),
        format!("https://u:{SENTINEL}@api.example"),
    ] {
        let mut file = sdk::load(&path).unwrap();
        let error = file
            .resolve_endpoints(&["ts".into()], |_| Some(value.clone()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("WEATHER_TS_ENDPOINT"), "{error}");
        assert!(!error.contains(SENTINEL), "{error}");
    }
}

/// A build script reads only the SDK file's model and names, so a file
/// `crucible sdk` would refuse never breaks an unrelated build.
#[test]
fn declared_reads_names_leniently() {
    let (directory, path) = repository(
        "schema_version = 99\nmodel = \"model.json\"\nfuture_key = 1\n[sdk.rust]\nprivate = true\n[sdk.ts]\n",
    );
    let declared = sdk::declared(&path).unwrap();
    assert_eq!(declared.model.as_deref(), Some("model.json"));
    assert_eq!(declared.names, ["rust", "ts"]);
    assert_eq!(
        sdk::find_in_repository(directory.path()),
        Some(path.clone())
    );
    fs::write(&path, "not = toml = at all").unwrap();
    assert_eq!(sdk::declared(&path), None);
    // Outside a Git repository nothing is found.
    let loose = tempfile::tempdir().unwrap();
    fs::write(loose.path().join(sdk::FILE_NAME), "model = \"m\"\n").unwrap();
    if !loose
        .path()
        .ancestors()
        .any(|ancestor| ancestor.join(".git").exists())
    {
        assert_eq!(sdk::find_in_repository(loose.path()), None);
    }
}

/// A path from the file displays with one separator on every platform.
#[test]
fn joined_paths_use_one_separator() {
    let (_directory, path) = repository("schema_version = 1\nmodel = \"model/api\"\n");
    let file = sdk::load(&path).unwrap();
    let joined = file.join("model/api").display().to_string();
    let other = if std::path::MAIN_SEPARATOR == '/' {
        '\\'
    } else {
        '/'
    };
    assert!(!joined.contains(other), "{joined}");
    assert!(joined.ends_with(&format!("model{}api", std::path::MAIN_SEPARATOR)));
}

/// A drive-prefixed, device-named or dot-trailing segment would escape or
/// alias on Windows: refused on every platform, for every path key.
#[test]
fn every_path_segment_is_a_plain_file_name() {
    for bad in [
        "gen/C:escaped2",
        "gen/D:pwned",
        "C:escaped",
        "gen.",
        "gen ",
        "gen/NUL",
        "gen/con.txt",
        "gen/COM1",
        "gen/lpt9.log",
        "gen/a|b",
    ] {
        let errors = sdk_errors(&format!(
            "[sdk.rust]\nout = \"{bad}\"\nreadme = \"{bad}\"\nlicense_file = \"{bad}\"\nruntime_path = \"{bad}\"\nlockfiles = [\"{bad}\"]\nchannel = {{ git = \"git@x:y\", path = \"{bad}\", adopt = [\"{bad}\"] }}\n"
        ));
        for key in [
            "sdk.rust.out",
            "sdk.rust.readme",
            "sdk.rust.license_file",
            "sdk.rust.runtime_path",
            "sdk.rust.lockfiles",
            "sdk.rust.channel.path",
            "sdk.rust.channel.adopt",
        ] {
            assert!(
                errors
                    .iter()
                    .any(|error| error.contains(&format!(": {key}: "))),
                "{bad:?} passes at {key}: {errors:#?}"
            );
        }
        let model = errors_of(&format!("schema_version = 1\nmodel = \"{bad}\"\n"));
        assert!(!model.is_empty(), "{bad:?} passes as the model");
    }
    // Ordinary names, dots inside included, stay fine.
    assert!(sdk_errors("[sdk.rust]\nout = \"gen/v1.2/rust-sdk\"\n").is_empty());
}

/// Names a case-insensitive filesystem folds together are one `out`.
#[test]
fn outs_that_fold_together_overlap() {
    assert_error(
        &sdk_errors(
            "[sdk.a]\nlanguage = \"rust\"\nout = \"ς\"\n[sdk.b]\nlanguage = \"rust\"\nout = \"σ\"\n",
        ),
        "sdk.b.out",
        "overlaps",
    );
}

/// Aliases the spelling cannot show (an 8.3 short name) are caught against
/// the filesystem before anything is written.
#[test]
fn outs_are_compared_as_the_filesystem_resolves_them() {
    let (directory, path) = repository(
        "schema_version = 1\nmodel = \"model.json\"\n[sdk.a]\nlanguage = \"rust\"\nout = \"generated-sdk\"\n[sdk.b]\nlanguage = \"rust\"\nout = \"GENERA~1\"\n",
    );
    let file = sdk::load(&path).unwrap();
    assert!(file.check_outs().is_ok());
    fs::create_dir(directory.path().join("generated-sdk")).unwrap();
    if directory.path().join("GENERA~1").exists() {
        let error = file.check_outs().unwrap_err();
        assert!(error.contains("overlapping"), "{error}");
    }
}

/// Single-slash and `git+` schemes carry credentials too, as Git and Cargo
/// read them; and a TOML syntax error never quotes the line it is on.
#[test]
fn credentials_in_every_url_spelling_are_refused_and_never_echoed() {
    let errors = sdk_errors(
        "[defaults]\nhomepage = \"https:/user:SECRETTOK@example.com/x\"\n[sdk.rust]\nout = \"r\"\nruntime = { git = \"https:/SECRETTOK2@github.com/o/r.git\", rev = \"r\", version = \"0.6\" }\nchannel = { git = \"git+https://SECRETTOK4@github.com/o/r.git\" }\n",
    );
    assert_error(&errors, "defaults.homepage", "credentials");
    assert_error(&errors, "sdk.rust.runtime.git", "user name");
    assert_error(&errors, "sdk.rust.channel.git", "user name");
    for error in &errors {
        assert!(!error.contains("SECRETTOK"), "{error}");
    }
    let syntax = errors_of("repository = \"https://u:SECRETTOK3@example.com\" x\n");
    assert_eq!(syntax.len(), 1);
    assert!(!syntax[0].contains("SECRETTOK3"), "{}", syntax[0]);
    assert!(syntax[0].contains(":1:"), "{}", syntax[0]);
}

fn errors_of(text: &str) -> Vec<String> {
    errors(text)
}

/// A private SDK is never published with public npm access.
#[test]
fn a_private_sdk_is_never_public_on_npm() {
    assert_error(
        &sdk_errors(
            "[sdk.ts]\npackage = \"x\"\nout = \"o\"\nprivate = true\nchannel = { npm = \"https://npm.pkg.github.com\", access = \"public\" }\n",
        ),
        "sdk.ts.channel.access",
        "access \"public\"",
    );
}
