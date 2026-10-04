use crucible_codegen_cpp::{Options, emit};
use crucible_smithy::Model;
use std::{fs, path::Path};

fn options() -> Options {
    Options {
        package_name: "weather_client".into(),
        namespace: "example::weather".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
        content_sha256: true,
    }
}
fn weather() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
}
fn lower(value: &serde_json::Value) -> crucible_smithy::ir::Service {
    Model::from_json(&value.to_string())
        .unwrap()
        .lower(None)
        .unwrap()
}
#[test]
fn weather_models_generate_deterministically() {
    let service = lower(&weather());
    let files = emit(&service, &options()).unwrap();
    let again = emit(&service, &options()).unwrap();
    let snapshot = files
        .iter()
        .filter(|file| {
            !file.path.contains("/runtime/")
                && ![
                    "src/json.cpp",
                    "src/timestamp.cpp",
                    "src/codec.cpp",
                    "src/transport.cpp",
                    "src/client_runtime.cpp",
                ]
                .contains(&file.path.as_str())
        })
        .map(|file| format!("=== {} ===\n{}\n", file.path, file.contents))
        .collect::<String>();
    let snapshot = format!("{}\n", snapshot.trim_end());
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/package.txt");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(golden.parent().unwrap()).unwrap();
        fs::write(&golden, &snapshot).unwrap();
    }
    assert_eq!(
        snapshot,
        fs::read_to_string(golden)
            .expect("golden")
            .replace("\r\n", "\n")
    );
    assert_eq!(
        files
            .iter()
            .map(|file| (&file.path, &file.contents))
            .collect::<Vec<_>>(),
        again
            .iter()
            .map(|file| (&file.path, &file.contents))
            .collect::<Vec<_>>()
    );
}
#[test]
fn package_namespace_and_version_are_validated() {
    let service = lower(&weather());
    for name in [
        "../escape",
        "weather-client",
        "all",
        "install",
        "",
        "3client",
        "class",
        "NUL",
        "con",
        "COM1",
        "LPT9",
    ] {
        let mut config = options();
        config.package_name = name.into();
        assert!(emit(&service, &config).is_err(), "{name}");
    }
    for name in [
        "std",
        "example::std",
        "example::",
        "a__b",
        "",
        "class",
        "NULL",
    ] {
        let mut config = options();
        config.namespace = name.into();
        assert!(emit(&service, &config).is_err(), "{name}");
    }
    for version in ["", "1..0", "1.2.3.4.5", "1.0-beta", "1)\nmessage(x)"] {
        let mut config = options();
        config.package_version = version.into();
        assert!(emit(&service, &config).is_err(), "{version}");
    }
}
#[test]
fn enum_union_and_member_collisions_are_diagnosed() {
    for (shape, name) in [
        ("Sky", "value"),
        ("Sky", "known_clear"),
        ("Location", "value"),
        ("CitySummary", "std"),
        ("CitySummary", "NULL"),
    ] {
        let mut model = weather();
        model["shapes"]["example.weather#Weather"]["rename"] =
            serde_json::json!({format!("example.weather#{shape}"):name});
        assert!(
            emit(&lower(&model), &options()).is_err(),
            "{shape} -> {name}"
        );
    }
    let mut model = weather();
    let members = model["shapes"]["example.weather#CreateCityInput"]["members"]
        .as_object_mut()
        .unwrap();
    members.insert(
        "class".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    members.insert(
        "class_value".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    assert!(emit(&lower(&model), &options()).is_err());
}

#[test]
fn head_request_bodies_are_diagnosed_before_generation() {
    let mut model = weather();
    model["shapes"]["example.weather#CreateCity"]["traits"]["smithy.api#http"]["method"] =
        "HEAD".into();
    let diagnostics = emit(&lower(&model), &options())
        .err()
        .expect("diagnostic")
        .to_string();
    assert!(diagnostics.contains("HEAD request body"));
}
