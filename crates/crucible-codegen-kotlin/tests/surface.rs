//! The surface names exactly what the generated package declares.

use std::path::{Path, PathBuf};

use crucible_codegen_kotlin::{Options, emit, surface};

fn model_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn options(endpoint: Option<&str>) -> Options {
    Options {
        group_id: "com.example".into(),
        artifact_id: "weather-client".into(),
        package_name: "example.weather".into(),
        package_version: "0.0.0-SNAPSHOT".into(),
        endpoint: endpoint.map(str::to_owned),
    }
}

#[test]
fn weather_surface_names_what_the_package_declares() {
    let service = crucible_lang::load_service(&model_dir("weather")).unwrap();
    let options = options(None);
    let files = emit(&service, &options).unwrap();
    let file = |path: &str| {
        files
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| panic!("no {path}"))
            .contents
            .as_str()
    };
    let surface = surface(&service, &options);
    assert_eq!(surface.language, "kotlin");
    assert_eq!(surface.package.ecosystem, "maven");
    assert_eq!(surface.package.name, "com.example:weather-client");
    assert_eq!(surface.package.import, "example.weather");
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(
        surface.client.construct,
        "WeatherClient(ClientConfig(endpoint = endpoint))"
    );
    let source = "src/commonMain/kotlin/example/weather";
    let client = file(&format!("{source}/WeatherClient.kt"));
    assert!(client.contains("class WeatherClient(config: ClientConfig)"));
    assert_eq!(surface.operations.len(), service.operations.len());
    let mut paginated = 0;
    for operation in &service.operations {
        let entry = &surface.operations[&operation.id.to_string()];
        assert!(
            client.contains(&format!("    suspend fun `{}`(", entry.method)),
            "{}",
            entry.method
        );
        assert_eq!(entry.async_method, None);
        assert_eq!(entry.paginators.is_some(), operation.paginated.is_some());
        for paginator in entry.paginators.iter().flatten() {
            paginated += 1;
            assert!(
                client.contains(&format!("    fun `{paginator}`(")),
                "{paginator}"
            );
        }
        for (structure, input) in [(&entry.input, true), (&entry.output, false)] {
            let Some(structure) = structure else { continue };
            assert_eq!(structure.style, if input { "dsl" } else { "properties" });
            let model = file(&format!("{source}/model/{}.kt", structure.type_name));
            assert!(model.contains(&format!(
                "class `{}` internal constructor(",
                structure.type_name
            )));
            for member in &structure.members {
                // Inputs are set on the DSL's builder; outputs are read
                // from the model's properties.
                let declaration = if input {
                    format!("        var `{}`: ", member.name)
                } else {
                    format!("    val `{}`: ", member.name)
                };
                assert!(model.contains(&declaration), "{declaration}");
            }
        }
    }
    assert!(paginated > 0, "weather has a paginated operation");
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "listCities");
    assert_eq!(
        list.paginators.as_deref(),
        Some(&["listCitiesPages".to_owned()][..])
    );
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, "ListCitiesInput");
    assert_eq!(input.members[2].name, "tags");
    assert_eq!(input.members[2].type_name, "Tags");
    assert_eq!(input.members[1].type_name, "Int");
}

#[test]
fn a_configured_endpoint_needs_no_construction_argument() {
    let service = crucible_lang::load_service(&model_dir("weather")).unwrap();
    let surface = surface(&service, &options(Some("https://weather.example")));
    assert_eq!(surface.client.construct, "WeatherClient(ClientConfig())");
    assert_eq!(surface.endpoint.as_deref(), Some("https://weather.example"));
}

/// Kotlin refuses colliding method names rather than suffixing them, so the
/// suffixed names other targets publish never reach a Kotlin package.
#[test]
fn colliding_operation_names_are_refused() {
    let service =
        crucible_lang::load_service(&model_dir("client-docs-surface-collisions")).unwrap();
    let Err(diagnostics) = emit(&service, &options(None)) else {
        panic!("colliding method names are refused");
    };
    assert!(
        diagnostics
            .to_string()
            .contains("Kotlin client method name collision"),
        "{diagnostics}"
    );
}
