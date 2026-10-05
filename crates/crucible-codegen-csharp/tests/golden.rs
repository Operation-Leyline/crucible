use crucible_codegen_csharp::{Options, emit};
use crucible_smithy::Model;
use std::{fs, path::Path};
fn options() -> Options {
    Options {
        namespace: "Example.Weather".into(),
        package_id: "Example.Weather".into(),
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
        if file.path.ends_with(".cs")
            && !file.path.starts_with("Models/")
            && !["WeatherClient.cs", "ModelRegistry.cs"].contains(&file.path.as_str())
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
fn configuration_diagnostics_are_collected() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let mut options = options();
    options.namespace = "../escape".into();
    options.package_id = "bad<id".into();
    assert_eq!(emit(&service, &options).err().unwrap().len(), 2);
}

#[test]
fn model_name_diagnostics_are_collected() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    model["shapes"]["example.weather#Weather"]["rename"] =
        serde_json::json!({"example.weather#CitySummary": "CON"});
    for name in ["message", "messageValue"] {
        model["shapes"]["example.weather#CreateCityInput"]["members"][name] =
            serde_json::json!({"target":"smithy.api#String"});
    }
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    assert_eq!(emit(&service, &options()).err().unwrap().len(), 2);
}

#[test]
fn service_client_name_is_reserved() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    model["shapes"]["example.weather#Weather"]["rename"] =
        serde_json::json!({"example.weather#CitySummary": "WeatherClient"});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    assert!(emit(&service, &options()).is_err());
}
