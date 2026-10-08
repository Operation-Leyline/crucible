#![cfg(feature = "rust")]
//! `Model::server().conformance()`: the generated test module is written beside
//! the service bindings, and a model that cannot supply one fails the build.

use std::path::Path;

use serde_json::Value;

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");
const VECTORS: &str = include_str!("../../crucible-smithy/tests/fixtures/weather-vectors.json");

#[test]
fn server_conformance_uses_the_full_model_for_either_audience() {
    let directory = tempfile::tempdir().unwrap();
    let mut model: Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/internal.json"
    ))
    .unwrap();
    for (operation, id, uri, params, headers, body) in [
        (
            "Public",
            "PrivateHeader",
            "/public",
            serde_json::json!({"privateHeader":"private"}),
            serde_json::json!({"x-private":"private"}),
            "{}",
        ),
        (
            "Secret",
            "InternalOperation",
            "/secret",
            serde_json::json!({"secret":"private"}),
            serde_json::json!({}),
            r#"{"secret":"private"}"#,
        ),
    ] {
        model["shapes"][format!("example.audience#{operation}")]["traits"]["smithy.test#httpRequestTests"] = serde_json::json!([{
            "id":id, "protocol":"crucible.protocols#json", "appliesTo":"server",
            "method":"POST", "uri":uri, "params":params, "headers":headers,
            "body":body, "bodyMediaType":"application/json"
        }]);
    }
    let path = directory.path().join("model.json");
    std::fs::write(&path, model.to_string()).unwrap();
    let render = |audience| {
        crucible_build::model(&path)
            .server()
            .audience(audience)
            .conformance()
            .render_conformance()
            .unwrap()
    };
    let external = render(crucible_build::Audience::External);
    assert_eq!(external, render(crucible_build::Audience::Internal));
    assert!(
        external.contains("conformance::request::<crate::api::Secret>"),
        "{external}"
    );
    assert!(external.contains("privateHeader"), "{external}");
}

/// The weather fixture with its `smithy.test` vectors, as a model file.
fn model(directory: &Path, with_vectors: bool) -> std::path::PathBuf {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    if with_vectors {
        let overlay: Value = serde_json::from_str(VECTORS).unwrap();
        for (shape, added) in overlay.as_object().unwrap() {
            let traits = model["shapes"][shape]["traits"].as_object_mut().unwrap();
            traits.extend(added.as_object().unwrap().clone());
        }
    }
    let path = directory.join("model.json");
    std::fs::write(&path, model.to_string()).unwrap();
    path
}

#[test]
fn conformance_tests_render_beside_the_service() {
    let directory = tempfile::tempdir().unwrap();
    let path = model(directory.path(), true);
    let tests = crucible_build::model(&path)
        .server()
        .conformance()
        .render_conformance()
        .unwrap();
    assert!(
        tests.contains("conformance::request::<crate::weather::GetCity>"),
        "{tests}"
    );
    assert!(!tests.contains("#[ignore"), "nothing is skipped by default");
}

#[test]
fn named_skips_become_ignored_tests_and_stale_ones_fail() {
    let directory = tempfile::tempdir().unwrap();
    let path = model(directory.path(), true);
    let tests = crucible_build::model(&path)
        .server()
        .skip_vector("GetCityOutput", "the server fails it")
        .render_conformance()
        .unwrap();
    assert!(
        tests.contains("#[ignore = \"the server fails it\"]"),
        "{tests}"
    );
    let error = crucible_build::model(&path)
        .server()
        .skip_vector("RemovedLongAgo", "stale")
        .render_conformance()
        .unwrap_err();
    assert!(
        error.contains("skip `RemovedLongAgo` names no vector"),
        "{error}"
    );
}

#[test]
fn a_model_without_vectors_fails_instead_of_testing_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let path = model(directory.path(), false);
    let error = crucible_build::model(&path)
        .server()
        .conformance()
        .render_conformance()
        .unwrap_err();
    assert!(error.contains("no smithy.test vector applies"), "{error}");
}

#[test]
fn conformance_is_off_unless_requested_and_is_server_only() {
    let directory = tempfile::tempdir().unwrap();
    let path = model(directory.path(), true);
    let error = crucible_build::model(&path)
        .server()
        .render_conformance()
        .unwrap_err();
    assert!(error.contains("call `.conformance()`"), "{error}");
}

#[test]
fn generating_writes_the_tests_into_out_dir() {
    // `generate` is a build-script call: it reads OUT_DIR and prints cargo
    // directives, so it runs in a child process like the other build probes.
    if std::env::var_os("CRUCIBLE_CONFORMANCE_PROBE").is_some() {
        crucible_build::model(std::env::var("CRUCIBLE_PROBE_MODEL").unwrap())
            .server()
            .conformance()
            .generate();
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let path = model(directory.path(), true);
    let out = directory.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "generating_writes_the_tests_into_out_dir",
            "--nocapture",
        ])
        .env("CRUCIBLE_CONFORMANCE_PROBE", "1")
        .env("CRUCIBLE_PROBE_MODEL", &path)
        .env("OUT_DIR", &out)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let written = std::fs::read_to_string(out.join("crucible_conformance.rs")).unwrap();
    assert!(written.contains("fn __conformance_build"), "{written}");
    assert!(out.join("crucible_service.rs").exists());
}
