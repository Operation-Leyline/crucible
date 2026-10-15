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
#[ignore = "documentation, infrastructure and Explorer output from a .cru package needs resource lowering (decision 63)"]
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
