use crucible_codegen_go::{Options, emit};
use crucible_smithy::Model;

#[test]
fn service_exception_is_exported_with_go_type_spelling() {
    let mut service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    service.id = crucible_smithy::ShapeId::new("example.weather", "weather_api");
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
    let runtime = &files
        .iter()
        .find(|file| file.path == "runtime.go")
        .unwrap()
        .contents;
    assert!(runtime.contains("type WeatherApiException interface"));
}
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
fn resource_only_types_do_not_change_sdk_files() {
    let model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    let generated = |model: &serde_json::Value| {
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        emit(
            &service,
            &Options {
                module_path: "example.com/weather".into(),
                package_name: "weather".into(),
                endpoint: None,
                content_sha256: false,
            },
        )
        .unwrap()
        .into_iter()
        .map(|file| (file.path, file.contents))
        .collect::<std::collections::BTreeMap<_, _>>()
    };
    let baseline = generated(&model);
    for resource_only_shape in [
        serde_json::json!({"type":"structure","members":{"value":{"target":"smithy.api#String"}}}),
        serde_json::json!({"type":"bigDecimal"}),
        serde_json::json!({"type":"intEnum","members":{"ZERO":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":0}}}}),
        serde_json::json!({"type":"set","member":{"target":"smithy.api#String"}}),
    ] {
        let mut extended = model.clone();
        extended["shapes"]["example.weather#ResourceOnlyIdentifier"] =
            serde_json::json!({"type":"string"});
        extended["shapes"]["example.weather#City"]["identifiers"]["deploymentId"] =
            serde_json::json!({"target":"example.weather#ResourceOnlyIdentifier"});
        extended["shapes"]["example.weather#ResourceOnly"] = resource_only_shape.clone();
        extended["shapes"]["example.weather#City"]["properties"] =
            serde_json::json!({"deploymentState":{"target":"example.weather#ResourceOnly"}});
        assert_eq!(
            generated(&extended),
            baseline,
            "resource-only {} must not affect any SDK file",
            resource_only_shape["type"]
        );
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
        if matches!(
            file.path.as_str(),
            "runtime.go"
                | "auth.go"
                | "query.go"
                | "waiters.go"
                | "endpoints.go"
                | "checksums.go"
                | "stream.go"
                | "policy.go"
                | "discovery.go"
                | "telemetry.go"
        ) {
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

fn stream_options() -> Options {
    Options {
        module_path: "example.com/stream".into(),
        package_name: "stream".into(),
        endpoint: None,
        content_sha256: false,
    }
}

#[test]
fn streaming_blobs_are_readers() {
    let service = Model::from_json(include_str!(
        "../../crucible-codegen-java/tests/streaming.json"
    ))
    .unwrap()
    .lower_with_capabilities(None, &crucible_codegen_go::capabilities())
    .unwrap();
    let files = emit(&service, &stream_options()).unwrap();
    let file = |name: &str| &files.iter().find(|f| f.path == name).unwrap().contents;
    let models = file("models.go");
    assert!(models.contains("import \"io\""));
    assert!(models.contains("type UploadInput struct {\n    Body io.Reader\n}"));
    assert!(models.contains("    Body io.ReadCloser\n    Count Optional[int32]"));
    assert!(!models.contains("type Stream ="), "{models}");
    assert!(file("client.go").contains("if err != nil { closeStreams(values) }"));
    assert!(files.iter().any(|f| f.path == "stream.go"));
}

#[test]
fn go_streams_outside_the_http_payload_are_diagnosed() {
    let model = serde_json::json!({"smithy":"2.0","shapes":{
        "test#Service":{"type":"service","version":"1","traits":{"aws.protocols#awsJson1_0":{}},"operations":[{"target":"test#Put"}]},
        "test#Put":{"type":"operation","input":{"target":"test#Input"}},
        "test#Stream":{"type":"blob","traits":{"smithy.api#streaming":{}}},
        "test#Input":{"type":"structure","members":{"body":{"target":"test#Stream","traits":{"smithy.api#required":{}}}}}
    }});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_go::capabilities())
        .unwrap();
    let error = emit(&service, &stream_options()).err().unwrap();
    assert!(format!("{error:?}").contains("not the streaming payload"));
}

#[test]
fn endpoint_discovery_models_generate_with_the_policy_runtime() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/runtime-policy.json"
    ))
    .unwrap()
    .lower_with_capabilities(None, &crucible_codegen_go::capabilities())
    .unwrap();
    let files = emit(
        &service,
        &Options {
            module_path: "example.com/policy".into(),
            package_name: "policy".into(),
            endpoint: None,
            content_sha256: false,
        },
    )
    .unwrap();
    for runtime in ["policy.go", "discovery.go", "telemetry.go"] {
        assert!(files.iter().any(|f| f.path == runtime), "{runtime}");
    }
    let client = &files
        .iter()
        .find(|f| f.path == "client.go")
        .unwrap()
        .contents;
    assert!(client.contains(r#"\"discoveredEndpoint\":{\"required\":true"#));
}

#[test]
fn runtime_policy_names_cannot_collide_with_models() {
    for name in [
        "RetryMode",
        "TelemetryHook",
        "AttemptFinishedEvent",
        "Clock",
    ] {
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
        let diagnostics = emit(&service, &stream_options()).err().unwrap();
        assert!(
            diagnostics
                .to_string()
                .contains("collides with another declaration"),
            "{name}"
        );
    }
}
