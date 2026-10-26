#![cfg(feature = "all-clients")]
//! Every language's generated `README.md` and `AGENTS.md` follow the
//! package's metadata: a configured README or `AGENTS.md` replaces the
//! generated one, a private or Git-released package never shows an install
//! from the public registry (which could fetch someone else's package of
//! that name), and a Git release is named by its tag.

use clients::clients;
use crucible_codegen::{Client, File, PackageMetadata};
use crucible_codegen_common::{GitRelease, Registry};
use std::path::Path;
use std::process::Command;

/// Exercise the declared SDK path, including its release-tag expansion.
fn rust_sdk(channel: &str) -> (tempfile::TempDir, crucible_codegen::sdk::SdkFile) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("README-rust.md"),
        "# Custom SDK guide\nUse HTTPS with your credential manager.\n",
    )
    .unwrap();
    let path = directory.path().join("crucible-sdk.toml");
    std::fs::write(
        &path,
        format!(
            r#"schema_version = 1
model = "model"
[sdk.rust]
package = "weather-sdk"
out = "generated/rust"
version = "1.2.3"
private = true
readme = "README-rust.md"
channel = {{ {channel} }}
"#
        ),
    )
    .unwrap();
    let file = crucible_codegen::sdk::load(&path).unwrap();
    (directory, file)
}

#[path = "common/clients.rs"]
mod clients;
mod common;

/// `client` with `metadata` in place of its own.
fn with(client: Client, metadata: &PackageMetadata) -> Client {
    let metadata = metadata.clone();
    match client {
        Client::Rust(mut o) => {
            o.metadata = metadata;
            Client::Rust(o)
        }
        Client::Ts(mut o) => {
            o.metadata = metadata;
            Client::Ts(o)
        }
        Client::Python(mut o) => {
            o.metadata = metadata;
            Client::Python(o)
        }
        Client::Go(mut o) => {
            o.metadata = metadata;
            Client::Go(o)
        }
        Client::Java(mut o) => {
            o.metadata = metadata;
            Client::Java(o)
        }
        Client::Csharp(mut o) => {
            o.metadata = metadata;
            Client::Csharp(o)
        }
        Client::Kotlin(mut o) => {
            o.metadata = metadata;
            Client::Kotlin(o)
        }
        Client::Swift(mut o) => {
            o.metadata = metadata;
            Client::Swift(o)
        }
        Client::Dart(mut o) => {
            o.metadata = metadata;
            Client::Dart(o)
        }
        Client::Php(mut o) => {
            o.metadata = metadata;
            Client::Php(o)
        }
        Client::Ruby(mut o) => {
            o.metadata = metadata;
            Client::Ruby(o)
        }
        Client::Cpp(mut o) => {
            o.metadata = metadata;
            Client::Cpp(o)
        }
        Client::C(mut o) => {
            o.metadata = metadata;
            Client::C(o)
        }
        Client::R(mut o) => {
            o.metadata = metadata;
            Client::R(o)
        }
    }
}

/// The package's README and AGENTS.md (R's under `inst/`).
fn guides(files: &[File]) -> (String, String) {
    let find = |name: &str| {
        files
            .iter()
            .find(|file| file.path == name || file.path == format!("inst/{name}"))
            .unwrap_or_else(|| panic!("no {name}"))
            .contents
            .clone()
    };
    (find("README.md"), find("AGENTS.md"))
}

/// What installs a package by name from a public registry.
const PUBLIC_INSTALLS: &[&str] = &[
    "npm install",
    "pip install",
    "gem install",
    "composer require",
    "dotnet add package",
    "PackageReference",
    "dart pub add",
    "cargo add",
];

#[test]
fn a_configured_readme_or_agents_replaces_the_generated_one() {
    let service = common::load("weather");
    let mut metadata = PackageMetadata::new();
    metadata.readme = Some("# Ours\n".into());
    metadata.agents = Some("# Our agents\n".into());
    for (language, client) in clients() {
        let files = with(client, &metadata).emit(&service).unwrap();
        let (readme, agents) = guides(&files);
        assert_eq!(readme, "# Ours\n", "{language}");
        assert_eq!(agents, "# Our agents\n", "{language}");
    }
}

#[test]
fn a_private_package_shows_no_public_registry_install() {
    let service = common::load("weather");
    let mut metadata = PackageMetadata::new();
    metadata.private = true;
    for (language, client) in clients() {
        let files = with(client, &metadata).emit(&service).unwrap();
        let (readme, agents) = guides(&files);
        for text in [&readme, &agents] {
            for install in PUBLIC_INSTALLS {
                assert!(!text.contains(install), "{language}: {install}\n{text}");
            }
        }
        if [
            "rust", "ts", "python", "csharp", "dart", "php", "ruby", "go",
        ]
        .contains(&language)
        {
            assert!(readme.contains("private"), "{language}:\n{readme}");
        }
    }
}

#[test]
fn a_git_release_is_installed_from_its_tag() {
    let service = common::load("weather");
    let mut metadata = PackageMetadata::new();
    metadata.registry = Some(Registry::Git);
    metadata.git = Some(GitRelease::new(
        "git@github.com:example/weather-sdks.git",
        "v1.0.0",
    ));
    for (language, client) in clients() {
        let files = with(client, &metadata).emit(&service).unwrap();
        let (readme, _) = guides(&files);
        for install in PUBLIC_INSTALLS {
            assert!(!readme.contains(install), "{language}: {install}\n{readme}");
        }
        let expected = match language {
            "rust" => {
                "hostile-sdk = { git = \"ssh://git@github.com/~/example/weather-sdks.git\", tag = \"v1.0.0\" }"
            }
            "go" => "go get example.test/hostile@v1.0.0",
            "swift" => ".package(url: \"git@github.com:example/weather-sdks.git\"",
            "ts" | "python" | "csharp" | "dart" | "php" | "ruby" => "Git tag `v1.0.0`",
            _ => continue,
        };
        assert!(
            readme.contains(expected),
            "{language}: {expected}\n{readme}"
        );
    }
}

#[test]
fn rust_git_guides_use_the_sdk_release_version_with_a_custom_readme() {
    let (_directory, file) =
        rust_sdk(r#"git = "https://github.com/example/weather-rs.git", tag = "sdk-v{version}""#);
    let client = file.sdks["rust"].client(&file, Some("2.3.4")).unwrap();
    let files = client.emit(&common::load("weather")).unwrap();
    let manifest: toml::Value = files
        .iter()
        .find(|file| file.path == "Cargo.toml")
        .unwrap()
        .contents
        .parse()
        .unwrap();
    assert_eq!(manifest["package"]["version"].as_str(), Some("2.3.4"));
    let (readme, agents) = guides(&files);
    assert_eq!(
        readme,
        "# Custom SDK guide\nUse HTTPS with your credential manager.\n"
    );
    assert!(agents.contains(
        r#"weather-sdk = { git = "https://github.com/example/weather-rs.git", tag = "sdk-v2.3.4" }"#
    ), "{agents}");
    assert!(!agents.contains("sdk-v1.2.3"), "{agents}");
    assert!(
        !agents.contains(&format!("sdk-v{}", env!("CARGO_PKG_VERSION"))),
        "{agents}"
    );
}

#[test]
fn rust_development_guides_do_not_invent_a_released_git_tag() {
    let (_directory, mut file) =
        rust_sdk(r#"git = "https://github.com/example/weather-rs.git", tag = "sdk-v{version}""#);
    for private in [false, true] {
        let metadata = &mut file.sdks.get_mut("rust").unwrap().metadata;
        metadata.private = private;
        metadata.readme = None;
        let client = file.sdks["rust"].client(&file, None).unwrap();
        let files = client.emit(&common::load("weather")).unwrap();
        let manifest: toml::Value = files
            .iter()
            .find(|file| file.path == "Cargo.toml")
            .unwrap()
            .contents
            .parse()
            .unwrap();
        assert_eq!(
            manifest["package"]["version"].as_str(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        let (readme, agents) = guides(&files);
        for text in [readme, agents] {
            assert!(!text.contains("sdk-v"), "{text}");
            assert!(!text.contains("weather-sdk = {"), "{text}");
            for install in PUBLIC_INSTALLS {
                assert!(!text.contains(install), "{install}\n{text}");
            }
            assert!(text.contains("development export"), "{text}");
            assert!(
                text.contains("installing `weather-sdk` from the public registry could fetch someone else's package"),
                "{text}"
            );
            assert!(
                text.contains(
                    "ask the SDK publisher for its distribution repository and release tag"
                ),
                "{text}"
            );
        }
    }
}

#[test]
fn rust_git_guides_use_the_explicit_install_url() {
    let (_directory, file) = rust_sdk(
        r#"git = "ssh://git@publish.example/weather.git", install_url = "https://download.example/weather.git", tag = "sdk-v{version}""#,
    );
    let sdk = &file.sdks["rust"];
    let effective = sdk.effective(&file);
    assert_eq!(
        effective["channel"]["git"],
        "ssh://git@publish.example/weather.git"
    );
    assert_eq!(
        effective["channel"]["install_url"],
        "https://download.example/weather.git"
    );
    let files = sdk
        .client(&file, Some("2.3.4"))
        .unwrap()
        .emit(&common::load("weather"))
        .unwrap();
    let (_, agents) = guides(&files);
    assert!(
        agents.contains(
            r#"weather-sdk = { git = "https://download.example/weather.git", tag = "sdk-v2.3.4" }"#
        ),
        "{agents}"
    );
    assert!(!agents.contains("publish.example"), "{agents}");
}

#[test]
fn rust_git_guides_preserve_unconfigured_remote_transports() {
    for (publish, install) in [
        (
            "git@internal.example:team/sdk.git",
            "ssh://git@internal.example/~/team/sdk.git",
        ),
        (
            "git@internal.example:~/team/sdk.git",
            "ssh://git@internal.example/~/team/sdk.git",
        ),
        (
            "git@internal.example:~other/team/sdk.git",
            "ssh://git@internal.example/~other/team/sdk.git",
        ),
        (
            "ssh://git@internal.example:2222/team/sdk.git",
            "ssh://git@internal.example:2222/team/sdk.git",
        ),
        (
            "git@internal.example:/team/sdk.git",
            "ssh://git@internal.example/team/sdk.git",
        ),
        (
            "https://internal.example/team/sdk.git",
            "https://internal.example/team/sdk.git",
        ),
    ] {
        let (_directory, mut file) =
            rust_sdk(&format!(r#"git = "{publish}", tag = "sdk-v{{version}}""#));
        file.sdks.get_mut("rust").unwrap().metadata.readme = None;
        let files = file.sdks["rust"]
            .client(&file, Some("2.3.4"))
            .unwrap()
            .emit(&common::load("weather"))
            .unwrap();
        let (readme, agents) = guides(&files);
        for text in [readme, agents] {
            assert!(
                text.contains(&format!(
                    r#"weather-sdk = {{ git = "{install}", tag = "sdk-v2.3.4" }}"#
                )),
                "{text}"
            );
        }
    }
}

#[test]
fn cargo_fetches_the_repository_named_by_the_generated_ssh_install_line() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let config = root.join("empty-git-config");
    std::fs::write(&config, "").unwrap();
    let ssh = root.join(if cfg!(windows) { "ssh.exe" } else { "ssh" });
    let mut compiler = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    compiler
        .arg("--edition=2024")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/git_ssh.rs"))
        .arg("-o")
        .arg(&ssh);
    checked(&mut compiler);
    for (category, version) in [("home", "1.2.3"), ("absolute", "9.8.7")] {
        let repository = root.join(category).join("team/sdk.git");
        std::fs::create_dir_all(repository.join("src")).unwrap();
        std::fs::write(
            repository.join("Cargo.toml"),
            format!("[package]\nname = \"weather-sdk\"\nversion = \"{version}\"\nedition = \"2024\"\n[workspace]\n"),
        ).unwrap();
        std::fs::write(
            repository.join("src/lib.rs"),
            "pub fn from_repository() {}\n",
        )
        .unwrap();
        for arguments in [
            vec!["init", "--quiet"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
            vec!["-c", "tag.gpgsign=false", "tag", "sdk-v2.3.4"],
        ] {
            checked(
                Command::new("git")
                    .args(arguments)
                    .current_dir(&repository)
                    .env("GIT_CONFIG_GLOBAL", &config)
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_COUNT", "0"),
            );
        }
    }
    for (index, (source, version)) in [
        ("git@fixture.invalid:team/sdk.git", "1.2.3"),
        ("git@fixture.invalid:/team/sdk.git", "9.8.7"),
        ("ssh://git@fixture.invalid:2222/~/team/sdk.git", "1.2.3"),
        ("ssh://git@fixture.invalid:2222/team/sdk.git", "9.8.7"),
    ]
    .into_iter()
    .enumerate()
    {
        let (_sdk_directory, mut file) =
            rust_sdk(&format!(r#"git = "{source}", tag = "sdk-v{{version}}""#));
        file.sdks.get_mut("rust").unwrap().metadata.readme = None;
        let files = file.sdks["rust"]
            .client(&file, Some("2.3.4"))
            .unwrap()
            .emit(&common::load("weather"))
            .unwrap();
        let (readme, _) = guides(&files);
        let dependency = readme
            .lines()
            .find(|line| line.starts_with("weather-sdk = {"))
            .unwrap();
        let consumer = root.join(format!("consumer-{index}"));
        std::fs::create_dir_all(consumer.join("src")).unwrap();
        std::fs::write(consumer.join("src/lib.rs"), "").unwrap();
        std::fs::write(consumer.join("Cargo.toml"), format!("[package]\nname = \"consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\n{dependency}\n")).unwrap();
        // The SSH executable serves the Git protocol from local fixtures,
        // so Cargo performs a real fetch without contacting a network host.
        // Default libgit2 SSH is qualified separately with a local SSH server.
        let output = checked(
            Command::new(env!("CARGO"))
                .args(["metadata", "--format-version", "1", "--manifest-path"])
                .arg(consumer.join("Cargo.toml"))
                .current_dir(&consumer)
                .env("CARGO_HOME", consumer.join("cargo-home"))
                .env("CARGO_NET_GIT_FETCH_WITH_CLI", "true")
                .env("CARGO_NET_RETRY", "0")
                .env("GIT_CONFIG_GLOBAL", &config)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_COUNT", "0")
                .env_remove("GIT_SSH_COMMAND")
                .env("GIT_SSH", &ssh)
                .env("GIT_SSH_VARIANT", "ssh")
                .env("SDK_SSH_ROOT", root)
                .env("SDK_SSH_LOG", consumer.join("ssh.log"))
                .env("RUSTC_WRAPPER", ""),
        );
        let metadata: serde_json::Value = serde_json::from_slice(&output).unwrap();
        let package = metadata["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|package| package["name"] == "weather-sdk")
            .unwrap();
        assert_eq!(package["version"], version, "{source}");
    }
}

fn checked(command: &mut Command) -> Vec<u8> {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{command:?}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn declared_git_development_guides_never_invent_published_tags() {
    let service = common::load("weather");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("crucible-sdk.toml");
    let mut config = "schema_version = 1\nmodel = \"model\"\n".to_owned();
    for (language, coordinates) in [
        ("rust", "package = \"weather-sdk\""),
        ("ts", "package = \"@example/weather\""),
        (
            "python",
            "package = \"weather-sdk\"\nmodule_name = \"weather_sdk\"",
        ),
        (
            "go",
            "package = \"weather\"\nmodule_path = \"example.test/weather\"",
        ),
        (
            "java",
            "package = \"example.weather\"\ngroup_id = \"example.weather\"\nartifact_id = \"weather-sdk\"",
        ),
        (
            "kotlin",
            "package = \"example.weather\"\ngroup_id = \"example.weather\"\nartifact_id = \"weather-sdk\"",
        ),
        (
            "csharp",
            "package_id = \"Weather.Sdk\"\nnamespace = \"Weather\"",
        ),
        (
            "swift",
            "package = \"WeatherSdk\"\nmodule_name = \"WeatherSdk\"",
        ),
        ("dart", "package = \"weather_sdk\""),
        (
            "php",
            "package = \"example/weather\"\nnamespace = \"Weather\"",
        ),
        (
            "ruby",
            "package = \"weather_sdk\"\nmodule_name = \"WeatherSdk\"",
        ),
        ("cpp", "package = \"weather\"\nnamespace = \"weather\""),
        ("c", "package = \"weather\"\nprefix = \"weather\""),
        ("r", "package = \"WeatherSdk\""),
    ] {
        config.push_str(&format!(
            "\n[sdk.{language}]\n{coordinates}\nout = \"generated/{language}\"\nversion = \"1.2.3\"\nchannel = {{ git = \"https://example.test/{language}.git\", tag = \"sdk-v{{version}}\" }}\n"
        ));
    }
    std::fs::write(&path, config).unwrap();
    let mut file = crucible_codegen::sdk::load(&path).unwrap();
    for private in [false, true] {
        for sdk in file.sdks.values_mut() {
            sdk.metadata.private = private;
        }
        for (language, sdk) in &file.sdks {
            let client = sdk.client(&file, None).unwrap();
            let files = client.emit(&service).unwrap();
            let (readme, agents) = guides(&files);
            for text in [readme, agents] {
                assert!(text.contains("development export"), "{language}: {text}");
                assert!(
                    text.contains("no assigned Git release tag"),
                    "{language}: {text}"
                );
                assert!(
                    text.contains("public registry could fetch someone else's package"),
                    "{language}: {text}"
                );
                assert!(text.contains("ask the SDK publisher"), "{language}: {text}");
                assert!(!text.contains("sdk-v"), "{language}: {text}");
                for install in PUBLIC_INSTALLS {
                    assert!(!text.contains(install), "{language}: {install}\n{text}");
                }
            }
            let released = sdk.client(&file, Some("2.3.4")).unwrap();
            let metadata = match &released {
                Client::Rust(o) => &o.metadata,
                Client::Ts(o) => &o.metadata,
                Client::Python(o) => &o.metadata,
                Client::Go(o) => &o.metadata,
                Client::Java(o) => &o.metadata,
                Client::Kotlin(o) => &o.metadata,
                Client::Csharp(o) => &o.metadata,
                Client::Swift(o) => &o.metadata,
                Client::Dart(o) => &o.metadata,
                Client::Php(o) => &o.metadata,
                Client::Ruby(o) => &o.metadata,
                Client::Cpp(o) => &o.metadata,
                Client::C(o) => &o.metadata,
                Client::R(o) => &o.metadata,
            };
            let git = metadata.git.as_ref().unwrap();
            assert_eq!(git.tag, "sdk-v2.3.4", "{language}");
            assert_eq!(git.url, format!("https://example.test/{language}.git"));
        }
    }
}

#[test]
fn a_scoped_npm_package_on_another_registry_configures_its_scope() {
    let service = common::load("weather");
    let mut metadata = PackageMetadata::new();
    metadata.registry = Some(Registry::npm("https://npm.pkg.github.com", None));
    let client = clients()
        .into_iter()
        .find(|(language, _)| *language == "ts")
        .unwrap()
        .1;
    let Client::Ts(mut options) = with(client, &metadata) else {
        unreachable!("the TypeScript client")
    };
    options.package_name = "@example/weather".into();
    let files = Client::Ts(options).emit(&service).unwrap();
    let (readme, agents) = guides(&files);
    for text in [&readme, &agents] {
        assert!(
            text.contains("npm config set @example:registry https://npm.pkg.github.com"),
            "{text}"
        );
    }
}
