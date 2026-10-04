use crucible_codegen_kotlin::{Options, emit};
use crucible_smithy::Model;
use std::{fs, path::Path};

fn options() -> Options {
    Options {
        group_id: "example".into(),
        artifact_id: "weather-client".into(),
        package_name: "example.weather".into(),
        package_version: "0.0.0-dev".into(),
        endpoint: Some("https://weather.example".into()),
        content_sha256: true,
    }
}

#[test]
fn weather_matches_goldens() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    for file in emit(&service, &options()).unwrap() {
        if file.path.ends_with(".kt")
            && !["Models.kt", "ModelCodecs.kt", "Client.kt"]
                .iter()
                .any(|name| file.path.ends_with(name))
        {
            continue;
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            file.contents,
            "{}",
            file.path
        );
    }
}

#[test]
fn rejects_jvm_accessor_and_union_collisions() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    for name in ["foo", "Foo", "isBar", "bar"] {
        model["shapes"]["example.weather#CreateCityInput"]["members"][name] =
            serde_json::json!({"target":"smithy.api#String"});
    }
    model["shapes"]["example.weather#Location"]["members"]["location"] =
        serde_json::json!({"target":"smithy.api#String"});
    model["shapes"]["example.weather#Location"]["members"]["models"] =
        serde_json::json!({"target":"smithy.api#Unit"});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    assert_eq!(emit(&service, &options()).err().unwrap().len(), 4);
    model["shapes"]["example.weather#Weather"]["rename"] =
        serde_json::json!({"example.weather#Location":"UnknownVariant"});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    assert_eq!(emit(&service, &options()).err().unwrap().len(), 4);
}

#[test]
fn configuration_and_get_body_are_diagnosed() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    model["shapes"]["example.weather#CreateCity"]["traits"]["smithy.api#http"]["method"] =
        "GET".into();
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let mut config = options();
    config.package_name = "../bad".into();
    config.group_id = "<bad>".into();
    assert_eq!(emit(&service, &config).err().unwrap().len(), 3);
}

#[test]
fn escapes_kotlin_literals_without_rewriting_literal_backslashes() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let mut config = options();
    config.endpoint = Some("https://example/\\foo/$bar/\u{000c}/\"/\n".into());
    let files = emit(&service, &config).unwrap();
    let client = files
        .iter()
        .find(|file| file.path.ends_with("/Client.kt"))
        .unwrap();
    assert!(
        client
            .contents
            .contains(r#""https://example/\\foo/\$bar/\u000c/\"/\n""#)
    );
}
