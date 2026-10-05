use crucible_codegen_go::{Options, emit};
use crucible_smithy::Model;
use std::{fs, path::Path};

#[test]
fn service_client_and_constructor_names_cannot_collide_with_models() {
    for name in ["WeatherClient", "NewWeatherClient"] {
        let mut model: serde_json::Value = serde_json::from_str(include_str!(
            "../../crucible-smithy/tests/fixtures/weather.json"
        ))
        .unwrap();
        model["shapes"]["example.weather#Weather"]["rename"] =
            serde_json::json!({"example.weather#CitySummary":name});
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        let diagnostics = emit(
            &service,
            &Options {
                module_path: "example.com/weather".into(),
                package_name: "weather".into(),
                endpoint: None,
                content_sha256: false,
            },
        )
        .err()
        .unwrap();
        assert!(
            diagnostics
                .to_string()
                .contains("collides with another declaration")
        );
    }
}

#[test]
fn invalid_package_configuration_is_collected() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let errors = emit(
        &service,
        &Options {
            module_path: "bad\nmodule".into(),
            package_name: "type".into(),
            endpoint: None,
            content_sha256: false,
        },
    )
    .err()
    .unwrap();
    assert_eq!(errors.len(), 2, "{errors}");
}

#[test]
fn accessor_names_do_not_collide_with_fields_or_other_accessors() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    let members = model["shapes"]["example.weather#CreateCityInput"]["members"]
        .as_object_mut()
        .unwrap();
    members.insert(
        "setName".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    members.insert(
        "getName".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let files = emit(
        &service,
        &Options {
            module_path: "example.com/weather".into(),
            package_name: "weather".into(),
            endpoint: None,
            content_sha256: false,
        },
    )
    .unwrap();
    let models = &files
        .iter()
        .find(|f| f.path == "models.go")
        .unwrap()
        .contents;
    assert!(models.contains("func (m *CreateCityInput) SetName2(value CityName)"));
    assert!(models.contains("func (m *CreateCityInput) GetName2() CityName"));
    assert!(models.contains("func (m *CreateCityInput) SetSetName(value string)"));
}

#[test]
fn weather_matches_goldens() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let files = emit(
        &service,
        &Options {
            module_path: "example.com/weather".into(),
            package_name: "weather".into(),
            endpoint: Some("https://weather.example".into()),
            content_sha256: true,
        },
    )
    .unwrap();
    for file in files {
        if file.path == "runtime.go" {
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
