//! The surface names exactly what the generated package declares.

use std::path::{Path, PathBuf};

use crucible_codegen_swift::{Options, emit, surface};

fn model_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn options(endpoint: Option<&str>) -> Options {
    Options {
        package_name: "WeatherSDK".into(),
        module_name: "Weather".into(),
        package_version: "0.0.0-dev".into(),
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
    assert_eq!(surface.language, "swift");
    assert_eq!(surface.package.ecosystem, "swiftpm");
    assert_eq!(surface.package.name, "WeatherSDK");
    assert_eq!(surface.package.import, "Weather");
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(
        surface.client.construct,
        "try WeatherClient(ClientConfig(endpoint: endpoint))"
    );
    let client = file("Sources/Weather/WeatherClient.swift");
    let models = file("Sources/Weather/Models.swift");
    assert!(client.contains("public final class WeatherClient: Sendable {"));
    assert_eq!(surface.operations.len(), service.operations.len());
    let mut paginated = 0;
    for operation in &service.operations {
        let entry = &surface.operations[&operation.id.to_string()];
        assert!(
            client.contains(&format!("    public func `{}`(_ input: ", entry.method)),
            "{}",
            entry.method
        );
        assert_eq!(entry.paginators.is_some(), operation.paginated.is_some());
        for paginator in entry.paginators.iter().flatten() {
            paginated += 1;
            assert!(
                client.contains(&format!("    public func `{paginator}`(_ input: ")),
                "{paginator}"
            );
        }
        for (structure, style) in [(&entry.input, "keywords"), (&entry.output, "properties")] {
            let Some(structure) = structure else { continue };
            assert_eq!(structure.style, style);
            let declaration = format!("    public struct `{}`: ", structure.type_name);
            let start = models
                .find(&declaration)
                .unwrap_or_else(|| panic!("{declaration}"));
            let end = start + models[start..].find("\n    }\n").unwrap();
            let body = &models[start..end];
            let init = body
                .lines()
                .find(|line| line.trim_start().starts_with("public init("))
                .unwrap();
            for member in &structure.members {
                assert!(
                    body.contains(&format!("        public var `{}`: ", member.name)),
                    "{}",
                    member.name
                );
                // Each member is an initializer label.
                assert!(init.contains(&format!("`{}`: ", member.name)), "{init}");
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
    assert_eq!(input.members[1].name, "maxResults");
    assert_eq!(input.members[1].type_name, "Int32");
}

#[test]
fn a_configured_endpoint_needs_no_construction_argument() {
    let service = crucible_lang::load_service(&model_dir("weather")).unwrap();
    let surface = surface(&service, &options(Some("https://weather.example")));
    assert_eq!(surface.client.construct, "try WeatherClient()");
    assert_eq!(surface.endpoint.as_deref(), Some("https://weather.example"));
}

/// Swift refuses colliding method names rather than suffixing them, so the
/// suffixed names other targets publish never reach a Swift package.
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
            .contains("Swift operation name collision"),
        "{diagnostics}"
    );
}
