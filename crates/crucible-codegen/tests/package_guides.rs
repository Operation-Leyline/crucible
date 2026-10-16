#![cfg(feature = "all-clients")]
//! Every language's generated `README.md` and `AGENTS.md` follow the
//! package's metadata: a configured README or `AGENTS.md` replaces the
//! generated one, a private or Git-released package never shows an install
//! from the public registry (which could fetch someone else's package of
//! that name), and a Git release is named by its tag.

use clients::clients;
use crucible_codegen::{Client, File, PackageMetadata};
use crucible_codegen_common::{GitRelease, Registry};

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
