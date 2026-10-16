//! SDK lines in every client language (docs/api-versions-clients.md, 5c;
//! decision 76): a package generated from release v1 of the e2e versioned
//! shop as the current model (v4) sees it records v1 as its
//! `serviceVersion` (schema 2 for Rust and TypeScript, with the line's own
//! `crucible-model.json`; schema 1 for the other twelve), and its
//! description names v1 as the release every request sends. `crucible.sdks/1`
//! names each line's surface as its generator does, checked in.
#![cfg(feature = "all-clients")]

use std::fs;
use std::path::{Path, PathBuf};

use crucible_codegen::sdk;
use crucible_ir::Audience;
use crucible_ir::ir::Service;

#[path = "common/clients.rs"]
mod clients;
mod common;

fn shop_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned")
}

/// The shop, lowered with its versions as a build lowers it.
fn shop() -> Service {
    crucible_lang::load_versioned_service(&shop_dir()).unwrap_or_else(|error| panic!("{error}"))
}

/// A file's text with JSON escapes and spaces taken out, so a description
/// embedded as a string literal reads as the JSON it holds.
fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '\\')
        .collect()
}

#[test]
fn every_client_records_and_sends_its_line() {
    let line = crucible_ir::versions::line(&shop(), "v1").unwrap();
    for (language, client) in clients::clients() {
        let files = client
            .emit_for_audience(&line, Audience::External)
            .unwrap_or_else(|diagnostics| panic!("{language}:\n{diagnostics}"));
        let record = files
            .iter()
            .find(|file| file.path.rsplit('/').next() == Some("crucible.json"))
            .unwrap_or_else(|| panic!("{language} writes crucible.json"));
        let record: serde_json::Value = serde_json::from_str(&record.contents).unwrap();
        assert_eq!(record["serviceVersion"], "v1", "{language}");
        if matches!(language, "rust" | "ts") {
            assert_eq!(record["schemaVersion"], 2, "{language}");
            let model = files
                .iter()
                .find(|file| file.path == "crucible-model.json")
                .unwrap_or_else(|| panic!("{language} writes crucible-model.json"));
            let model = crucible_ir::snapshot::read(&model.contents).unwrap();
            assert_eq!(model.version, "v1", "{language}");
            assert!(
                model.versions.is_empty(),
                "{language}: no chain in a snapshot"
            );
        }
        // The release every request names: the shared description's
        // `apiVersion` for the bundled runtimes, each operation's spec for
        // TypeScript, the operation marker for Rust.
        let sent = |release: &str| {
            files.iter().any(|file| {
                let text = plain(&file.contents);
                text.contains(&format!("\"apiVersion\":\"{release}\""))
                    || text.contains(&format!("apiVersion:\"{release}\""))
                    || text.contains(&format!("Option::Some(\"{release}\")"))
            })
        };
        assert!(sent("v1"), "{language} sends v1");
        assert!(!sent("v4"), "{language} sends only v1");
    }
}

#[test]
fn every_line_names_its_surface() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    common::copy_package(&shop_dir(), &directory.path().join("model"));
    let mut text = String::from("schema_version = 1\nmodel = \"model\"\n");
    for (language, keys) in [
        ("rust", ""),
        ("ts", "package = \"@example/shop-v1\"\n"),
        (
            "python",
            "package = \"shop-v1\"\nmodule_name = \"shop_v1\"\n",
        ),
        (
            "go",
            "package = \"shop\"\nmodule_path = \"example.com/shop/v1\"\n",
        ),
        (
            "java",
            "package = \"com.example.shop.v1\"\ngroup_id = \"com.example\"\nartifact_id = \"shop-v1\"\n",
        ),
        (
            "kotlin",
            "package = \"com.example.shop.v1\"\ngroup_id = \"com.example\"\nartifact_id = \"shop-v1-kotlin\"\n",
        ),
        (
            "csharp",
            "package_id = \"Example.Shop.V1\"\nnamespace = \"Example.Shop.V1\"\n",
        ),
        ("swift", "package = \"ShopV1\"\nmodule_name = \"ShopV1\"\n"),
        ("dart", "package = \"shop_v1\"\n"),
        (
            "php",
            "package = \"example/shop-v1\"\nnamespace = \"Example\\\\ShopV1\"\n",
        ),
        (
            "ruby",
            "package = \"example_shop_v1\"\nmodule_name = \"ExampleShopV1\"\n",
        ),
        (
            "cpp",
            "package = \"shop_v1\"\nnamespace = \"example::shop_v1\"\n",
        ),
        ("c", "package = \"shop_v1\"\nprefix = \"shop_v1\"\n"),
        ("r", "package = \"shopv1\"\n"),
    ] {
        text.push_str(&format!(
            "\n[sdk.{language}-v1]\nlanguage = \"{language}\"\napi_version = \"v1\"\n{keys}out = \"generated/{language}-v1\"\n"
        ));
    }
    let path = directory.path().join(sdk::FILE_NAME);
    fs::write(&path, &text).unwrap();
    let file = sdk::load(&path).unwrap_or_else(|errors| panic!("{errors}"));
    let current = shop();
    // A reference for v1 describes the v1 lines; the current one describes
    // none of them.
    let line = crucible_ir::versions::line(&current, "v1").unwrap();
    let described = crucible_codegen::sdks::file(&file, &line, Audience::External).unwrap();
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/shop-v1.sdks.json");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::write(&golden, &described.contents).unwrap();
    }
    assert_eq!(
        fs::read_to_string(&golden)
            .expect("golden exists; bless with CRUCIBLE_BLESS=1")
            .replace("\r\n", "\n"),
        described.contents
    );
    let document: serde_json::Value = serde_json::from_str(&described.contents).unwrap();
    let sdks = document["sdks"].as_object().unwrap();
    assert_eq!(sdks.len(), 14);
    for (name, sdk) in sdks {
        let operations = sdk["operations"].as_object().unwrap();
        // What v1 had that v3 removed, and what v3 and v4 added.
        for id in ["CountItems", "ListLabels", "GetLabel", "PutLabel"] {
            assert!(
                operations.contains_key(&format!("example.shop#{id}")),
                "{name}: {id}"
            );
        }
    }
    let current_reference =
        crucible_codegen::sdks::file(&file, &current, Audience::External).unwrap();
    let document: serde_json::Value = serde_json::from_str(&current_reference.contents).unwrap();
    assert!(document["sdks"].as_object().unwrap().is_empty());
    // `crucible sdk describe` describes every line it is given, each from
    // its own release, whatever the model it is handed.
    let every: Vec<&sdk::Sdk> = file.sdks.values().collect();
    let all = crucible_codegen::sdks::document(&file, &current, &every).unwrap();
    assert_eq!(
        all["sdks"],
        serde_json::from_str::<serde_json::Value>(&described.contents).unwrap()["sdks"]
    );
}
