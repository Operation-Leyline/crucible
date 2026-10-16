#![cfg(feature = "docs")]

mod common;

use std::fs;
use std::process::Command;

use crucible_build::Audience;
use serde_json::Value;

#[test]
fn docs_probe() {
    if std::env::var_os("CRUCIBLE_DOCS_PROBE").is_none() {
        return;
    }
    let model = crucible_build::model("service-docs-weather");
    let external = model.docs("reference").generate_into("exports");
    assert_eq!(external.target, "docs");
    let data: Value =
        serde_json::from_slice(&fs::read(external.path.join("describe.json")).unwrap()).unwrap();
    assert_eq!(data["format"], "crucible.describe/1");
    assert!(data["resources"]["example.weather#City"].is_object());
    assert!(data["operations"].get("example.weather#Status").is_none());
    let page = fs::read("exports/src/operations/API_GetCity.md").unwrap();
    let timestamp = fs::metadata("exports/describe.json")
        .unwrap()
        .modified()
        .unwrap();
    let sibling = fs::read("exports/keep.txt").unwrap();
    let rebuilt = crucible_build::model("service-docs-weather")
        .docs("reference")
        .generate_into("exports");
    assert_eq!(
        fs::read(rebuilt.path.join("src/operations/API_GetCity.md")).unwrap(),
        page
    );
    assert_eq!(
        fs::metadata("exports/describe.json")
            .unwrap()
            .modified()
            .unwrap(),
        timestamp
    );
    assert_eq!(fs::read("exports/keep.txt").unwrap(), sibling);
    assert_eq!(
        fs::read(external.path.join("describe.json")).unwrap(),
        fs::read("exports/describe.json").unwrap()
    );
    let internal = model
        .docs("internal")
        .audience(Audience::Internal)
        .generate();
    assert!(
        fs::read_to_string(internal.path.join("src/operations/API_Status.md"))
            .unwrap()
            .contains("Internal API")
    );
    let index: Value =
        serde_json::from_slice(&fs::read("out/crucible-artifacts.json").unwrap()).unwrap();
    assert!(index.is_object());
}

#[test]
fn docs_share_artifact_ownership_stable_exports_and_failure_preservation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::install_shared(root, "service-docs-weather");
    fs::create_dir(root.join("exports")).unwrap();
    fs::write(root.join("exports/keep.txt"), "consumer-owned").unwrap();
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "docs_probe", "--nocapture"])
        .env("CRUCIBLE_DOCS_PROBE", "1")
        .env("CARGO_MANIFEST_DIR", root)
        .env("OUT_DIR", root.join("out"))
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn docs_sdks_probe() {
    if std::env::var_os("CRUCIBLE_DOCS_SDKS_PROBE").is_none() {
        return;
    }
    let model = crucible_build::model("service-docs-weather");
    let reference = model.docs("reference").sdks("crucible-sdk.toml").generate();
    let sdks: Value =
        serde_json::from_slice(&fs::read(reference.path.join("sdks.json")).unwrap()).unwrap();
    assert_eq!(sdks["format"], "crucible.sdks/1");
    assert_eq!(sdks["service"], "example.weather#Weather");
    // The internal SDK is not an external reference's to name.
    assert_eq!(
        sdks["sdks"].as_object().unwrap().keys().collect::<Vec<_>>(),
        ["rust"]
    );
    assert_eq!(
        sdks["sdks"]["rust"]["operations"]["example.weather#ListCities"]["paginators"][0],
        "list_cities_pages"
    );
    assert!(reference.files.iter().any(|file| file == "sdks.json"));
    // Beside the description it accompanies, naming the same operations.
    let description: Value =
        serde_json::from_slice(&fs::read(reference.path.join("describe.json")).unwrap()).unwrap();
    for id in sdks["sdks"]["rust"]["operations"]
        .as_object()
        .unwrap()
        .keys()
    {
        assert!(description["operations"].get(id).is_some(), "{id}");
    }
    let internal = model
        .docs("internal")
        .audience(Audience::Internal)
        .sdks("crucible-sdk.toml")
        .generate();
    let sdks: Value =
        serde_json::from_slice(&fs::read(internal.path.join("sdks.json")).unwrap()).unwrap();
    assert_eq!(sdks["sdks"].as_object().unwrap().len(), 2);
}

#[test]
fn docs_write_sdks_json_beside_describe_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::install_shared(root, "service-docs-weather");
    fs::write(
        root.join("crucible-sdk.toml"),
        "schema_version = 1\nmodel = \"service-docs-weather\"\n\n[sdk.rust]\nout = \"generated/rust\"\n\n[sdk.rust-internal]\nlanguage = \"rust\"\naudience = \"internal\"\npackage = \"weather-internal\"\nout = \"generated/rust-internal\"\n",
    )
    .unwrap();
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "docs_sdks_probe", "--nocapture"])
        .env("CRUCIBLE_DOCS_SDKS_PROBE", "1")
        .env("CARGO_MANIFEST_DIR", root)
        .env("OUT_DIR", root.join("out"))
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn docs_history_probe() {
    if std::env::var_os("CRUCIBLE_DOCS_HISTORY_PROBE").is_none() {
        return;
    }
    let reference = crucible_build::model("version-history")
        .docs("reference")
        .generate();
    let read = |name: &str| -> Value {
        serde_json::from_slice(&fs::read(reference.path.join(name)).unwrap()).unwrap()
    };
    let description = read("describe.json");
    assert_eq!(description["versions"]["current"], "v3");
    assert_eq!(
        description["versions"]["releases"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        description["operations"]["example.history#DeleteItem"]["history"]["added"],
        "v3"
    );
    let changes = read("changes.json");
    assert_eq!(changes["format"], "crucible.changes/1");
    assert_eq!(changes["versions"], description["versions"]);
    assert!(reference.files.iter().any(|file| file == "changes.json"));
    assert!(reference.path.join("src/releases/v3.md").is_file());
}

/// A build's reference reads the release chain the build serves: every
/// supported release and what changed in each, beside `describe.json`.
#[test]
fn docs_document_the_release_chain() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::install_shared(root, "version-history");
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "docs_history_probe", "--nocapture"])
        .env("CRUCIBLE_DOCS_HISTORY_PROBE", "1")
        .env("CARGO_MANIFEST_DIR", root)
        .env("OUT_DIR", root.join("out"))
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
