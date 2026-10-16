//! The surface (`crucible.sdks/1`) names what the generated package
//! declares: every operation function, page iterator, model and accessor.

use crucible_codegen_c::{Options, emit, surface};
use crucible_ir::ir::Service;

fn options() -> Options {
    Options {
        package_name: "weather_client".into(),
        prefix: "weather".into(),
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

fn file<'a>(files: &'a [crucible_codegen_c::File], path: &str) -> &'a str {
    &files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("no {path}"))
        .contents
}

/// Asserts that every name the surface gives is declared in the header
/// the generator emits for `service`, with the type the surface states.
fn assert_declared(service: &Service) -> crucible_codegen_common::surface::Surface {
    let options = options();
    let files = emit(service, &options).unwrap();
    let client = file(&files, "include/weather_client/client.h");
    let models = file(&files, "include/weather_client/models.h");
    let surface = surface(service, &options);
    assert!(client.contains(&format!(
        "weather_status {name}_new(const {name}_config *config, {name} **output",
        name = surface.client.name
    )));
    assert_eq!(surface.operations.len(), service.operations.len());
    let declares = |name: &str| client.contains(&format!("\nweather_status {name}("));
    for operation in &service.operations {
        let entry = &surface.operations[&operation.id.to_string()];
        assert!(declares(&entry.method), "{}", entry.method);
        for paginator in entry.paginators.iter().flatten() {
            assert!(declares(paginator), "{paginator}");
        }
        assert_eq!(entry.paginators.is_some(), operation.paginated.is_some());
        if let Some(input) = &entry.input {
            assert_eq!(input.style, "setters");
            assert!(models.contains(&format!("\nstruct {} {{", input.type_name)));
            for member in &input.members {
                let setter = format!(
                    "static inline void {}({} *model, {} value)",
                    member.name, input.type_name, member.type_name
                );
                assert!(models.contains(&setter), "{setter}");
            }
        }
        if let Some(output) = &entry.output {
            assert_eq!(output.style, "functions");
            assert!(models.contains(&format!("\nstruct {} {{", output.type_name)));
            for member in &output.members {
                let getter = format!(
                    "static inline {} {}(const {} *model)",
                    member.type_name, member.name, output.type_name
                );
                assert!(models.contains(&getter), "{getter}");
            }
        }
    }
    surface
}

#[test]
fn weather_surface_names_the_generated_declarations() {
    let surface = assert_declared(&load("weather"));
    assert_eq!(surface.language, "c");
    assert_eq!(surface.package.ecosystem, "cmake");
    assert_eq!(surface.package.name, "weather_client");
    assert_eq!(surface.package.import, "weather_client/client.h");
    assert_eq!(surface.client.name, "weather_client");
    assert_eq!(
        surface.client.construct,
        "weather_client_config config = weather_client_defaults(); config.endpoint = weather_string_view(endpoint); weather_client *client = NULL; weather_error error = {0}; weather_client_new(&config, &client, &error);"
    );
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "weather_operations_list_cities");
    assert_eq!(
        list.paginators,
        Some(vec![
            "weather_operations_list_cities_pages_new".to_owned(),
            "weather_operations_list_cities_pages_next".to_owned(),
        ])
    );
    let output = list.output.as_ref().unwrap();
    assert_eq!(output.type_name, "weather_models_list_cities_output");
}

#[test]
fn a_default_endpoint_needs_no_configuration() {
    let service = load("weather");
    let mut options = options();
    options.endpoint = Some("https://api.weather.example".into());
    let surface = surface(&service, &options);
    assert!(!surface.client.construct.contains("endpoint"));
    assert_eq!(
        surface.endpoint.as_deref(),
        Some("https://api.weather.example")
    );
}

/// C's `_pages_new`/`_pages_next` suffixes leave the derived names other
/// generators collide on (`ListThingsPages`, `PaginateListThings`) free, so
/// the fixture generates, and the surface names what it declares.
#[test]
fn derived_names_beside_colliding_operations_are_declared() {
    let surface = assert_declared(&load("client-docs-surface-collisions"));
    let list = &surface.operations["example.surface#ListThings"];
    assert_eq!(
        list.paginators,
        Some(vec![
            "weather_operations_list_things_pages_new".to_owned(),
            "weather_operations_list_things_pages_next".to_owned(),
        ])
    );
    assert_eq!(
        surface.operations["example.surface#ListThingsPages"].method,
        "weather_operations_list_things_pages"
    );
}
