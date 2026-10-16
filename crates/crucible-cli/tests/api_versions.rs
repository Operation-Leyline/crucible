//! SDK lines from the CLI (docs/api-versions-clients.md, 5c; decision 76):
//! `crucible generate <output> --api-version <release>` and an SDK file
//! entry's `api_version` generate every output but a server from the
//! current model as that release sees it, lowered as a build lowers it. The
//! package is the e2e versioned shop: v4 current, v1 to v3 frozen.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod common;

fn crucible(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_crucible"))
        .current_dir(directory)
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

fn json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// A service repository holding the versioned shop as `shop/`.
fn repository() -> tempfile::TempDir {
    let directory = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    let shop =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    common::copy_package(&shop, &directory.path().join("shop"));
    directory
}

#[test]
fn generate_writes_an_older_releases_line() {
    let directory = repository();
    let root = directory.path();
    success(crucible(
        root,
        &[
            "generate",
            "ts",
            "shop",
            "--api-version",
            "v1",
            "--out",
            "ts-v1",
            "--package-name",
            "@example/shop-v1",
        ],
    ));
    assert_eq!(
        json(&root.join("ts-v1/crucible.json"))["serviceVersion"],
        "v1"
    );
    let schemas = fs::read_to_string(root.join("ts-v1/src/schemas.ts")).unwrap();
    assert!(schemas.contains(r#"apiVersion: "v1""#), "{schemas}");
    assert!(!schemas.contains(r#"apiVersion: "v4""#), "{schemas}");
    let models = fs::read_to_string(root.join("ts-v1/src/models.ts")).unwrap();
    // v1's names: `max`, renamed `limit` in v2.
    assert!(models.contains("max?: number;"), "{models}");
    // The current release is the default.
    success(crucible(
        root,
        &[
            "generate",
            "ts",
            "shop",
            "--out",
            "ts",
            "--package-name",
            "@example/shop",
        ],
    ));
    assert_eq!(json(&root.join("ts/crucible.json"))["serviceVersion"], "v4");

    // A language with a schema-1 record writes the line's release too.
    success(crucible(
        root,
        &[
            "generate",
            "python",
            "shop",
            "--api-version",
            "v2",
            "--out",
            "python-v2",
            "--package-name",
            "shop-v2",
            "--module-name",
            "shop_v2",
        ],
    ));
    let record = walk(&root.join("python-v2"))
        .into_iter()
        .find(|path| path.file_name().is_some_and(|name| name == "crucible.json"))
        .expect("the Python package records its provenance");
    assert_eq!(json(&record)["serviceVersion"], "v2");
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

#[test]
fn documents_and_openapi_describe_the_line() {
    let directory = repository();
    let root = directory.path();
    success(crucible(
        root,
        &[
            "generate",
            "docs",
            "shop",
            "--api-version",
            "v1",
            "--out",
            "docs-v1",
        ],
    ));
    let description = json(&root.join("docs-v1/describe.json"));
    assert_eq!(description["service"]["version"], "v1");
    assert_eq!(description["versions"]["current"], "v4");
    assert!(description["operations"]["example.shop#CountItems"].is_object());
    success(crucible(
        root,
        &[
            "generate",
            "openapi",
            "shop",
            "--api-version",
            "v1",
            "--out",
            "openapi.json",
        ],
    ));
    assert_eq!(json(&root.join("openapi.json"))["info"]["version"], "v1");
}

#[test]
fn a_server_is_never_generated_from_a_line() {
    let directory = repository();
    let root = directory.path();
    let error = failure(crucible(
        root,
        &[
            "generate",
            "rust",
            "shop",
            "--api-version",
            "v1",
            "--out",
            "server.rs",
        ],
    ));
    assert!(
        error.contains("a server serves every release from the current model"),
        "{error}"
    );
    assert!(!root.join("server.rs").exists());
    let error = failure(crucible(
        root,
        &[
            "scaffold",
            "shop",
            "--api-version",
            "v1",
            "--out",
            "operations",
        ],
    ));
    assert!(error.contains("scaffold without --api-version"), "{error}");
    // The Rust client of a line is generated like any other.
    success(crucible(
        root,
        &[
            "generate",
            "rust",
            "shop",
            "--api-version",
            "v1",
            "--client",
            "--out",
            "rust-v1",
        ],
    ));
    let lib = fs::read_to_string(root.join("rust-v1/src/lib.rs")).unwrap();
    assert!(lib.contains(r#"Some("v1")"#), "the client sends v1");
    // So is the current release's server.
    success(crucible(
        root,
        &[
            "generate",
            "rust",
            "shop",
            "--api-version",
            "v4",
            "--out",
            "server.rs",
        ],
    ));
}

#[test]
fn a_release_outside_the_chain_names_the_supported_ones() {
    let directory = repository();
    let root = directory.path();
    let error = failure(crucible(
        root,
        &[
            "generate",
            "ts",
            "shop",
            "--api-version",
            "v9",
            "--out",
            "ts",
            "--package-name",
            "x",
        ],
    ));
    assert!(
        error.contains(r#"--api-version v9: example.shop#Shop: release "v9" is not one example.shop#Shop serves: its releases are "v1", "v2", "v3", "v4""#),
        "{error}"
    );
    assert!(!root.join("ts").exists());
    // `check` checks the line it names.
    success(crucible(root, &["check", "shop", "--api-version", "v1"]));
    failure(crucible(root, &["check", "shop", "--api-version", "v0"]));
}

const SDK_FILE: &str = r#"schema_version = 1
model = "shop"

[sdk.ts]
package = "@example/shop"
out = "generated/ts"

[sdk.ts-v1]
language = "ts"
api_version = "v1"
package = "@example/shop-v1"
out = "generated/ts-v1"

[sdk.rust-v2]
language = "rust"
api_version = "v2"
out = "generated/rust-v2"
"#;

/// Each SDK file entry is a line of its own release: `sdk generate`
/// writes each from its projection, `sdk config` shows the key, and
/// `sdk describe` names each line's surface.
#[test]
fn an_sdk_file_entry_names_its_release() {
    let directory = repository();
    let root = directory.path();
    fs::write(root.join("crucible-sdk.toml"), SDK_FILE).unwrap();
    success(crucible(root, &["sdk", "generate"]));
    let version = |sdk: &str| {
        json(&root.join(format!("generated/{sdk}/crucible.json")))["serviceVersion"].clone()
    };
    assert_eq!(version("ts"), "v4");
    assert_eq!(version("ts-v1"), "v1");
    assert_eq!(version("rust-v2"), "v2");
    let models = fs::read_to_string(root.join("generated/ts-v1/src/models.ts")).unwrap();
    assert!(models.contains("max?: number;"), "{models}");

    let config: serde_json::Value =
        serde_json::from_str(&success(crucible(root, &["sdk", "config", "--json"]))).unwrap();
    assert_eq!(config["sdks"]["ts-v1"]["api_version"], "v1");
    assert_eq!(config["sdks"]["ts"]["api_version"], serde_json::Value::Null);
    let text = success(crucible(root, &["sdk", "config", "--sdk", "ts-v1"]));
    assert!(text.contains("api_version"), "{text}");

    let document: serde_json::Value =
        serde_json::from_str(&success(crucible(root, &["sdk", "describe"]))).unwrap();
    let operations = |sdk: &str| {
        document["sdks"][sdk]["operations"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };
    // v1 and v2 still have the operation v3 removed; v4 does not.
    assert!(operations("ts-v1").contains(&"example.shop#CountItems".to_owned()));
    assert!(operations("rust-v2").contains(&"example.shop#CountItems".to_owned()));
    assert!(!operations("ts").contains(&"example.shop#CountItems".to_owned()));
    // SDKs of several releases: the raw request is the current release's.
    assert_eq!(
        document["http"]["operations"]["example.shop#GetItem"]["uri"],
        "/things/id"
    );

    // One line described alone: its raw request is its release's, with
    // v1's route, names and header, as the line's SDK sends them.
    let line: serde_json::Value = serde_json::from_str(&success(crucible(
        root,
        &["sdk", "describe", "--sdk", "ts-v1"],
    )))
    .unwrap();
    let http = &line["http"]["operations"];
    assert_eq!(http["example.shop#GetItem"]["uri"], "/items/id", "{http}");
    assert!(
        http["example.shop#ListItems"]["uri"]
            .as_str()
            .unwrap()
            .contains("max="),
        "{http}"
    );
    assert!(
        http["example.shop#GetItem"]["headers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|header| header["name"] == "Crucible-Version" && header["value"] == "v1"),
        "{http}"
    );

    // A release the model does not serve is refused at its key.
    fs::write(
        root.join("crucible-sdk.toml"),
        SDK_FILE.replace(r#"api_version = "v1""#, r#"api_version = "v0""#),
    )
    .unwrap();
    let error = failure(crucible(root, &["sdk", "generate", "--sdk", "ts-v1"]));
    assert!(error.contains("sdk.ts-v1.api_version"), "{error}");
    assert!(error.contains(r#"release "v0""#), "{error}");
}

/// A reference describes the SDKs of its own release: `sdks.json` beside a
/// v1 reference lists the v1 line, and the current reference the current.
#[test]
fn a_reference_describes_the_sdks_of_its_release() {
    let directory = repository();
    let root = directory.path();
    fs::write(root.join("crucible-sdk.toml"), SDK_FILE).unwrap();
    for (release, out, expected, lines) in [
        (
            None,
            "docs",
            vec!["ts"],
            serde_json::json!({ "v1": ["ts-v1"], "v2": ["rust-v2"] }),
        ),
        (
            Some("v1"),
            "docs-v1",
            vec!["ts-v1"],
            serde_json::json!({ "v2": ["rust-v2"], "v4": ["ts"] }),
        ),
        (
            Some("v2"),
            "docs-v2",
            vec!["rust-v2"],
            serde_json::json!({ "v1": ["ts-v1"], "v4": ["ts"] }),
        ),
    ] {
        let mut args = vec![
            "generate",
            "docs",
            "shop",
            "--out",
            out,
            "--sdks",
            "crucible-sdk.toml",
        ];
        if let Some(release) = release {
            args.extend(["--api-version", release]);
        }
        success(crucible(root, &args));
        let sdks = json(&root.join(out).join("sdks.json"));
        let names: Vec<&String> = sdks["sdks"].as_object().unwrap().keys().collect();
        assert_eq!(names, expected, "{out}");
        // The other releases' SDKs, by release, so a site knows they exist.
        assert_eq!(sdks["lines"], lines, "{out}");
    }
}
