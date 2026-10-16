//! The surface (`crucible.sdks/1`) names what the generated package
//! declares: every client method, page function, model constructor and
//! argument.

use crucible_codegen_r::{Options, emit, surface};
use crucible_ir::ir::Service;

fn options() -> Options {
    Options {
        package_name: "weather.client".into(),
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

fn file<'a>(files: &'a [crucible_codegen_r::File], path: &str) -> &'a str {
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
    let client = file(&files, "R/client.R");
    let surface = surface(&service, &options);
    assert_eq!(surface.language, "r");
    assert_eq!(surface.package.ecosystem, "cran");
    assert_eq!(surface.package.name, "weather.client");
    assert_eq!(surface.package.import, "weather.client");
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(
        surface.client.construct,
        "WeatherClient(client_config(endpoint = endpoint))"
    );
    assert!(client.contains(&format!(
        "`{}` <- function(config = client_config())",
        surface.client.name
    )));
    assert_eq!(surface.operations.len(), service.operations.len());
    let declares = |name: &str| client.contains(&format!("  client$`{name}` <- function(input"));
    let mut paginated = 0;
    for operation in &service.operations {
        let entry = &surface.operations[&operation.id.to_string()];
        assert!(declares(&entry.method), "{}", entry.method);
        for paginator in entry.paginators.iter().flatten() {
            paginated += 1;
            assert!(declares(paginator), "{paginator}");
        }
        assert_eq!(entry.paginators.is_some(), operation.paginated.is_some());
        for (structure, style) in entry
            .input
            .iter()
            .map(|input| (input, "keywords"))
            .chain(entry.output.iter().map(|output| (output, "elements")))
        {
            assert_eq!(structure.style, style);
            // The constructor's arguments are the value's elements.
            let model = file(&files, &format!("R/model-{}.R", structure.type_name));
            let constructor = model
                .lines()
                .find(|line| line.starts_with(&format!("`{}` <- function(", structure.type_name)))
                .unwrap_or_else(|| panic!("constructor {}", structure.type_name));
            for member in &structure.members {
                assert!(
                    constructor.contains(&format!("\"{0}\" = `{0}`", member.name)),
                    "{constructor} has {}",
                    member.name
                );
            }
        }
    }
    assert!(paginated > 0, "weather has a paginated operation");
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "list_cities");
    assert_eq!(list.paginators, Some(vec!["list_cities_pages".to_owned()]));
    let create = surface.operations["example.weather#CreateCity"]
        .input
        .as_ref()
        .unwrap();
    let revision = create
        .members
        .iter()
        .find(|member| member.member == "expectedRevision")
        .unwrap();
    assert_eq!(
        (revision.name.as_str(), revision.type_name.as_str()),
        ("expected_revision", "crucible_long")
    );
}

#[test]
fn a_default_endpoint_needs_no_configuration() {
    let service = load("weather");
    let mut options = options();
    options.endpoint = Some("https://api.weather.example".into());
    let surface = surface(&service, &options);
    assert_eq!(surface.client.construct, "WeatherClient()");
    assert_eq!(
        surface.endpoint.as_deref(),
        Some("https://api.weather.example")
    );
}

/// R diagnoses a page function that collides with an operation's method
/// instead of renaming it, so a surface never names a renamed method.
#[test]
fn colliding_derived_methods_are_refused() {
    let service = load("client-docs-surface-collisions");
    let diagnostics = emit(&service, &options()).err().expect("refused");
    assert!(
        diagnostics
            .to_string()
            .contains("R operation method collision"),
        "{diagnostics}"
    );
}
