mod common;

use std::fs;
use std::process::Command;

use serde_json::Value;

#[test]
fn docs_cli_projects_audiences_and_preserves_outputs_on_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::install_shared(root, "service-docs-weather");
    let model = root.join("service-docs-weather");
    let out = root.join("reference");
    let run = |audience: &str| {
        Command::new(env!("CARGO_BIN_EXE_crucible"))
            .args(["generate", "docs"])
            .arg(&model)
            .arg("--out")
            .arg(&out)
            .args(["--audience", audience])
            .output()
            .unwrap()
    };
    let result = run("internal");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(out.join("src/operations/API_Status.md").is_file());
    fs::write(out.join("keep.txt"), "consumer-owned").unwrap();
    let result = run("external");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!out.join("src/operations/API_Status.md").exists());
    let bytes = fs::read(out.join("describe.json")).unwrap();
    let describe: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(describe["format"], "crucible.describe/1");
    assert!(describe["resources"]["example.weather#City"].is_object());
    assert_eq!(
        fs::read_to_string(out.join("keep.txt")).unwrap(),
        "consumer-owned"
    );
    fs::remove_dir_all(&model).unwrap();
    fs::create_dir(&model).unwrap();
    let result = run("external");
    assert!(!result.status.success());
    assert_eq!(fs::read(out.join("describe.json")).unwrap(), bytes);
}

/// `generate docs` reads the release chain as a build does: the
/// reference dates elements and lists each release's changes, in
/// `describe.json` and in `changes.json` beside it.
#[test]
fn docs_cli_documents_the_release_chain() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("reference");
    let result = Command::new(env!("CARGO_BIN_EXE_crucible"))
        .args(["generate", "docs"])
        .arg(common::shared_package("version-history"))
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let read = |name: &str| -> Value {
        serde_json::from_slice(&fs::read(out.join(name)).unwrap()).unwrap()
    };
    let describe = read("describe.json");
    let names: Vec<&str> = describe["versions"]["releases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|release| release["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["v1", "v2", "v3"]);
    assert_eq!(
        describe["shapes"]["example.history#Size"]["members"][1]["history"],
        serde_json::json!({"added": "v3", "changed": [], "missing": ["v1"]})
    );
    let changes = read("changes.json");
    assert_eq!(changes["format"], "crucible.changes/1");
    assert_eq!(changes["versions"], describe["versions"]);
    assert!(out.join("src/releases/v2.md").is_file());
}

/// `--schemas` moves the operations' JSON Schemas into one root map, or
/// leaves them out; every mode is `crucible.describe/1`.
#[test]
fn docs_cli_shares_or_omits_operation_schemas() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::install_shared(root, "service-docs-weather");
    let model = root.join("service-docs-weather");
    let describe = |schemas: &str| -> Value {
        let out = root.join(schemas);
        let result = Command::new(env!("CARGO_BIN_EXE_crucible"))
            .args(["generate", "docs"])
            .arg(&model)
            .arg("--out")
            .arg(&out)
            .args(["--schemas", schemas])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice(&fs::read(out.join("describe.json")).unwrap()).unwrap()
    };
    let inline = describe("inline");
    let get = "example.weather#GetCity";
    assert!(inline["operations"][get]["inputSchema"]["$defs"].is_object());
    assert!(inline.get("schemas").is_none());
    let shared = describe("shared");
    assert_eq!(shared["format"], "crucible.describe/1");
    assert_eq!(
        shared["operations"][get]["outputSchema"],
        serde_json::json!({"$ref": "#/$defs/example.weather.GetCityOutput"})
    );
    assert_eq!(
        shared["schemas"]["$defs"]["example.weather.GetCityOutput"]["required"],
        serde_json::json!(["city"])
    );
    let omitted = describe("omit");
    assert!(omitted["operations"][get].get("inputSchema").is_none());
    assert_eq!(omitted["schemas"], serde_json::json!({"mode": "omit"}));
}

/// `--openapi` writes the same API as an OpenAPI document beside the
/// reference, for a site to offer as a download; without it there is none,
/// and one written before is removed with the other generated files.
#[test]
fn docs_cli_writes_an_openapi_document_on_request() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::install_shared(root, "service-docs-weather");
    let model = root.join("service-docs-weather");
    let out = root.join("reference");
    let run = |extra: &[&str]| {
        let result = Command::new(env!("CARGO_BIN_EXE_crucible"))
            .args(["generate", "docs"])
            .arg(&model)
            .arg("--out")
            .arg(&out)
            .args(extra)
            .output()
            .unwrap();
        (
            result.status.success(),
            String::from_utf8_lossy(&result.stderr).into_owned(),
        )
    };
    let (ok, stderr) = run(&["--openapi", "--endpoint", "https://api.example.com"]);
    assert!(ok, "{stderr}");
    let document: Value =
        serde_json::from_slice(&fs::read(out.join("openapi.json")).unwrap()).unwrap();
    assert!(
        document["openapi"].as_str().unwrap().starts_with("3.1"),
        "{}",
        document["openapi"]
    );
    assert_eq!(document["servers"][0]["url"], "https://api.example.com");
    // The document is the external audience's, as the reference is.
    let describe: Value =
        serde_json::from_slice(&fs::read(out.join("describe.json")).unwrap()).unwrap();
    let operations = describe["operations"].as_object().unwrap().len();
    let documented: usize = document["paths"]
        .as_object()
        .unwrap()
        .values()
        .map(|path| path.as_object().unwrap().len())
        .sum();
    assert_eq!(documented, operations);

    let (ok, stderr) = run(&[]);
    assert!(ok, "{stderr}");
    assert!(!out.join("openapi.json").exists());
    // The server URL belongs to the document it names.
    let (ok, stderr) = run(&["--endpoint", "https://api.example.com"]);
    assert!(!ok);
    assert!(stderr.contains("--openapi"), "{stderr}");
}
