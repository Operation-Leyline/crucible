#![cfg(feature = "all-clients")]
//! Every language's generated `README.md` and `AGENTS.md` follow the
//! package's metadata: a configured README or `AGENTS.md` replaces the
//! generated one, a private or Git-released package never shows an install
//! from the public registry (which could fetch someone else's package of
//! that name), and a Git release is named by its tag.

use clients::clients;
use crucible_codegen::{Client, File, PackageMetadata};
use crucible_codegen_common::{GitRelease, Registry};

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
                "hostile-sdk = { git = \"ssh://git@github.com/example/weather-sdks.git\", tag = \"v1.0.0\" }"
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
    let (_directory, file) =
        rust_sdk(r#"git = "https://github.com/example/weather-rs.git", tag = "sdk-v{version}""#);
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
    let (_, agents) = guides(&files);
    assert!(
        !agents.contains(&format!("sdk-v{}", env!("CARGO_PKG_VERSION"))),
        "{agents}"
    );
    assert!(!agents.contains("sdk-v1.2.3"), "{agents}");
    assert!(!agents.contains("cargo add weather-sdk"), "{agents}");
    assert!(agents.contains("development export"), "{agents}");
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
