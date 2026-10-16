//! The surface names exactly what the generated package declares.

use std::path::{Path, PathBuf};

use crucible_codegen_csharp::{Options, emit, surface};

fn model_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn options(endpoint: Option<&str>) -> Options {
    Options {
        namespace: "Example.Weather".into(),
        package_id: "Example.Weather.Client".into(),
        package_version: "0.0.0-dev".into(),
        endpoint: endpoint.map(str::to_owned),
    }
}

/// Whether a `public` line of `source` declares the member `name`.
fn declares(source: &str, name: &str) -> bool {
    source.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("public ")
            && (line.contains(&format!(" @{name}(")) || line.contains(&format!(" @{name} {{")))
    })
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
    assert_eq!(surface.language, "csharp");
    assert_eq!(surface.package.ecosystem, "nuget");
    assert_eq!(surface.package.name, "Example.Weather.Client");
    assert_eq!(surface.package.import, "Example.Weather");
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(
        surface.client.construct,
        "new WeatherClient(new ClientConfig { Endpoint = endpoint })"
    );
    assert_eq!(surface.endpoint, None);
    let client = file("WeatherClient.cs");
    assert!(client.contains("public sealed class WeatherClient "));
    assert_eq!(surface.operations.len(), service.operations.len());
    let mut paginated = 0;
    for operation in &service.operations {
        let entry = &surface.operations[&operation.id.to_string()];
        assert!(declares(client, &entry.method), "{}", entry.method);
        let async_method = entry.async_method.as_deref().unwrap();
        assert!(declares(client, async_method), "{async_method}");
        assert_eq!(entry.paginators.is_some(), operation.paginated.is_some());
        for paginator in entry.paginators.iter().flatten() {
            paginated += 1;
            assert!(declares(client, paginator), "{paginator}");
        }
        for (structure, style) in [(&entry.input, "fields"), (&entry.output, "properties")] {
            let Some(structure) = structure else { continue };
            assert_eq!(structure.style, style);
            let model = file(&format!("Models/{}.cs", structure.type_name));
            assert!(model.contains(&format!("public sealed class @{} ", structure.type_name)));
            for member in &structure.members {
                assert!(declares(model, &member.name), "{}", member.name);
            }
        }
    }
    assert!(paginated > 0, "weather has a paginated operation");
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "ListCities");
    assert_eq!(list.async_method.as_deref(), Some("ListCitiesAsync"));
    assert_eq!(
        list.paginators.as_deref(),
        Some(&["ListCitiesPagesAsync".to_owned()][..])
    );
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, "ListCitiesInput");
    assert_eq!(input.members[0].name, "NextToken");
    assert_eq!(input.members[0].type_name, "string");
}

#[test]
fn a_configured_endpoint_needs_no_construction_argument() {
    let service = crucible_lang::load_service(&model_dir("weather")).unwrap();
    let surface = surface(&service, &options(Some("https://weather.example")));
    assert_eq!(
        surface.client.construct,
        "new WeatherClient(new ClientConfig())"
    );
    assert_eq!(surface.endpoint.as_deref(), Some("https://weather.example"));
}

/// C# refuses colliding method names rather than suffixing them, so the
/// suffixed names other targets publish never reach a C# package.
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
            .contains("C# client method name collision"),
        "{diagnostics}"
    );
}
