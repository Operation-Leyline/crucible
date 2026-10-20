#![cfg(feature = "json-schema")]
use crucible_build::{Audience, Generator, JsonSchemaOptions};
use serde_json::Value;
use std::{fs, process::Command};
mod common;
#[test]
fn json_schema_build_probe() {
    if std::env::var_os("CRUCIBLE_JSON_SCHEMA_PROBE").is_none() {
        return;
    }
    let model = crucible_build::model("json-schema").compile();
    let artifact = model
        .json_schema("plan")
        .shape("RenderPlan")
        .generate_into("exports/plan");
    assert_eq!(artifact.target, "json-schema");
    assert_eq!(artifact.files, ["schema.json"]);
    let exported = fs::read_to_string("exports/plan/schema.json").unwrap();
    assert_eq!(
        exported,
        fs::read_to_string(artifact.path.join("schema.json")).unwrap()
    );
    let value: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        value["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert!(value["properties"]["source"].is_object());
    let package = crucible_build::model("json-schema").compile();
    let err = package
        .json_schema("plan")
        .shape("Secret")
        .try_generate_into("exports/plan")
        .unwrap_err();
    assert!(err.contains("removed by audience projection"), "{err}");
    assert_eq!(
        exported,
        fs::read_to_string("exports/plan/schema.json").unwrap()
    );
    package
        .json_schema("secret")
        .audience(Audience::Internal)
        .options(JsonSchemaOptions {
            shape: "Secret".into(),
        })
        .generate();
    let direct = crucible_build::model("json-schema")
        .json_schema("direct")
        .shape("example.schema#RenderPlan")
        .try_generate()
        .unwrap();
    assert_eq!(
        fs::read_to_string(direct.path.join("schema.json")).unwrap(),
        exported
    );
    let service = crucible_lang::load_service(&common::shared_package("json-schema")).unwrap();
    let files = Generator::JsonSchema(JsonSchemaOptions {
        shape: "RenderPlan".into(),
    })
    .emit(&service)
    .unwrap();
    assert_eq!(files[0].path, "schema.json");
    assert_eq!(files[0].contents, exported);
}
#[test]
fn named_schema_build_output_exports_and_protects_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    common::install_shared(dir.path(), "json-schema");
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "json_schema_build_probe", "--nocapture"])
        .env("CRUCIBLE_JSON_SCHEMA_PROBE", "1")
        .env("CARGO_MANIFEST_DIR", dir.path())
        .env("OUT_DIR", dir.path().join("out"))
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
