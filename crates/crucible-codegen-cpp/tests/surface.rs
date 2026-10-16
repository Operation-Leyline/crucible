//! The surface (`crucible.sdks/1`) names what the generated package
//! declares: every client method, paginator, model and field.

use crucible_codegen_common::surface::unqualified;
use crucible_codegen_cpp::{Options, emit, surface};
use crucible_ir::ir::Service;

fn options() -> Options {
    Options {
        package_name: "weather_client".into(),
        namespace: "weather".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
    }
}

fn load(name: &str) -> Service {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&dir).unwrap()
}

fn file<'a>(files: &'a [crucible_codegen_cpp::File], path: &str) -> &'a str {
    &files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("no {path}"))
        .contents
}

#[test]
fn weather_surface_names_the_generated_declarations() {
    let service = load("weather");
    let options = options();
    let files = emit(&service, &options).unwrap();
    let client = file(&files, "include/weather_client/client.hpp");
    let models = file(&files, "include/weather_client/models.hpp");
    let surface = surface(&service, &options);
    assert_eq!(surface.language, "cpp");
    assert_eq!(surface.package.ecosystem, "cmake");
    assert_eq!(surface.package.name, "weather_client");
    assert_eq!(surface.package.import, "weather_client/client.hpp");
    assert_eq!(surface.client.name, "WeatherClient");
    assert!(client.contains(&format!("class {} {{", surface.client.name)));
    assert_eq!(
        surface.client.construct,
        "weather::WeatherClient(weather::runtime::ClientConfig{.endpoint = endpoint})"
    );
    assert_eq!(surface.operations.len(), service.operations.len());
    let declares_method = |name: &str| {
        client.lines().any(|line| {
            line.trim_start().starts_with("[[nodiscard]]") && line.contains(&format!(" {name}("))
        })
    };
    let mut paginated = 0;
    for operation in &service.operations {
        let entry = &surface.operations[&operation.id.to_string()];
        assert!(declares_method(&entry.method), "{}", entry.method);
        for paginator in entry.paginators.iter().flatten() {
            paginated += 1;
            assert!(declares_method(paginator), "{paginator}");
        }
        assert_eq!(entry.paginators.is_some(), operation.paginated.is_some());
        for structure in entry.input.iter().chain(&entry.output) {
            assert_eq!(structure.style, "fields");
            let start = models
                .find(&format!("\nstruct {} {{", structure.type_name))
                .unwrap_or_else(|| panic!("struct {}", structure.type_name));
            let body = &models[start..start + models[start..].find("\n};").unwrap()];
            for member in &structure.members {
                let declaration = body
                    .lines()
                    .find(|line| line.ends_with(&format!(" {};", member.name)))
                    .unwrap_or_else(|| panic!("{}.{}", structure.type_name, member.name));
                assert!(
                    unqualified(declaration, "::").contains(&member.type_name),
                    "{declaration} declares {}",
                    member.type_name
                );
            }
        }
    }
    assert!(paginated > 0, "weather has a paginated operation");
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "list_cities");
    assert_eq!(list.paginators, Some(vec!["list_cities_pages".to_owned()]));
}

#[test]
fn a_default_endpoint_needs_no_configuration() {
    let service = load("weather");
    let mut options = options();
    options.endpoint = Some("https://api.weather.example".into());
    let surface = surface(&service, &options);
    assert_eq!(surface.client.construct, "weather::WeatherClient()");
    assert_eq!(
        surface.endpoint.as_deref(),
        Some("https://api.weather.example")
    );
}

/// C++ diagnoses a derived method that collides with an operation's
/// instead of suffixing it, so a surface never names a renamed method.
#[test]
fn colliding_derived_methods_are_refused() {
    let service = load("client-docs-surface-collisions");
    let diagnostics = emit(&service, &options()).err().expect("refused");
    assert!(
        diagnostics
            .to_string()
            .contains("C++ operation name collision"),
        "{diagnostics}"
    );
}
