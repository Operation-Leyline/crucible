#![cfg(feature = "rust")]
//! SDK lines from a build script (docs/api-versions-clients.md, 5c;
//! decision 76): `model(..).api_version("v1")` generates every output but a
//! server from the current model as release v1 sees it. The package is the
//! e2e versioned shop: v4 current, v1 to v3 frozen and served.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[test]
fn api_version_probe() {
    let Ok(mode) = std::env::var("CRUCIBLE_API_VERSION_PROBE") else {
        return;
    };
    match mode.as_str() {
        "line" => {
            let package = crucible_build::model("shop").api_version("v1").compile();
            assert_eq!(package.api_version(), "v1");
            // The in-crate Rust client names v1 in Crucible-Version and has
            // v1's shapes.
            let code = crucible_build::model("shop")
                .api_version("v1")
                .client()
                .render()
                .unwrap();
            assert!(
                code.contains(r#"::std::option::Option::Some("v1")"#),
                "{code}"
            );
            assert!(code.contains("CountItems"), "removed in v3, still v1's");
            #[cfg(feature = "ts")]
            {
                let artifact = package
                    .ts_client("ts-v1")
                    .package("@example/shop-v1")
                    .generate();
                let read = |path: &str| fs::read_to_string(artifact.path.join(path)).unwrap();
                assert!(read("src/schemas.ts").contains(r#"apiVersion: "v1""#));
                assert!(!read("src/schemas.ts").contains(r#"apiVersion: "v4""#));
                assert!(read("src/models.ts").contains("max?: number;"));
                let record: serde_json::Value =
                    serde_json::from_str(&read("crucible.json")).unwrap();
                assert_eq!(record["serviceVersion"], "v1");
                let model = crucible_ir::snapshot::read(&read("crucible-model.json")).unwrap();
                assert_eq!(model.version, "v1");
            }
            #[cfg(feature = "docs")]
            {
                let artifact = package.docs("docs-v1").generate();
                let text = fs::read_to_string(artifact.path.join("describe.json")).unwrap();
                let description: serde_json::Value = serde_json::from_str(&text).unwrap();
                // The reference of a release says which it is, and which is
                // current (decision 79).
                assert_eq!(description["service"]["version"], "v1");
                assert_eq!(description["versions"]["current"], "v4");
                let releases: Vec<&str> = description["versions"]["releases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|release| release["name"].as_str().unwrap())
                    .collect();
                assert_eq!(releases, ["v1", "v2", "v3", "v4"]);
                assert!(description["operations"]["example.shop#CountItems"].is_object());
            }
            // The current release is the default, and names itself.
            let current = crucible_build::model("shop").api_version("v4").compile();
            assert_eq!(current.api_version(), "v4");
            assert_eq!(crucible_build::model("shop").compile().api_version(), "v4");
        }
        "server" => {
            let error = crucible_build::model("shop")
                .api_version("v1")
                .server()
                .render()
                .unwrap_err();
            println!("{error}");
            // A server from the current release is the server.
            crucible_build::model("shop")
                .api_version("v4")
                .server()
                .render()
                .unwrap();
        }
        "unknown" => {
            let error = crucible_build::model("shop")
                .api_version("v9")
                .try_compile()
                .err()
                .unwrap();
            println!("{error}");
        }
        other => panic!("unknown probe {other}"),
    }
}

fn run(root: &Path, mode: &str) -> Output {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "api_version_probe", "--nocapture"])
        .current_dir(root)
        .env("CRUCIBLE_API_VERSION_PROBE", mode)
        .env("CARGO_MANIFEST_DIR", root)
        .env("OUT_DIR", root.join("out"))
        .output()
        .unwrap()
}

fn shop() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("out")).unwrap();
    let versioned =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    common::copy_package(&versioned, &root.path().join("shop"));
    root
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_build_generates_an_older_releases_line() {
    let root = shop();
    let output = run(root.path(), "line");
    assert!(output.status.success(), "{}", text(&output));
}

#[test]
fn a_server_is_never_generated_from_a_line() {
    let root = shop();
    let output = run(root.path(), "server");
    let text = text(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("a server serves every release from the current model"),
        "{text}"
    );
}

#[test]
fn a_release_outside_the_chain_names_the_supported_ones() {
    let root = shop();
    let output = run(root.path(), "unknown");
    let text = text(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains(r#"release "v9" is not one example.shop#Shop serves: its releases are "v1", "v2", "v3", "v4""#),
        "{text}"
    );
}
