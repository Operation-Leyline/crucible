//! The surface (`crucible.sdks/1`) names what the generated package
//! declares: every client method (calling its own operation), page
//! function, exported model constructor and argument, and the constructor
//! and configuration a client is built with.

use crucible_codegen_common::surface::Surface;
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

/// Asserts that every name the surface gives is declared and exported by
/// the package the generator emits for `service` with `options`.
fn assert_declared(service: &Service, options: &Options) -> Surface {
    let files = emit(service, options).unwrap();
    let client = file(&files, "R/client.R");
    let namespace = file(&files, "NAMESPACE");
    let runtime = file(&files, "R/runtime.R");
    let surface = surface(service, options);
    let package = &options.package_name;
    assert_eq!(surface.package.import, *package);
    assert_eq!(surface.package.imports, [format!("library({package})")]);
    assert_eq!(surface.package.qualifier, "");
    assert!(!surface.client.statements && !surface.client.fallible);
    let exported = |name: &str| namespace.contains(&format!("export(\"{name}\")\n"));

    // The exported client constructor and the configuration it takes.
    let name = &surface.client.name;
    assert!(exported(name), "{name}");
    assert!(client.contains(&format!(
        "`{name}` <- function(config = client_config()) {{\n  client <- .crucible_client(config, "
    )));
    assert!(exported("client_config"));
    assert!(runtime.contains("client_config <- function(endpoint = NULL, "));
    match &options.endpoint {
        Some(endpoint) => {
            assert_eq!(surface.client.construct, format!("{name}()"));
            assert!(client.contains(&format!(".crucible_client(config, \"{endpoint}\")")));
        }
        None => {
            assert_eq!(
                surface.client.construct,
                format!("{name}(client_config(endpoint = endpoint))")
            );
            assert!(client.contains(".crucible_client(config, NULL)"));
        }
    }

    assert_eq!(surface.operations.len(), service.operations.len());
    for operation in &service.operations {
        let entry = &surface.operations[&operation.id.to_string()];
        // Each method dispatches to its own operation by wire name.
        let method = |name: &str, dispatch: &str| {
            let default = if entry.input.is_none() { " = NULL" } else { "" };
            format!(
                "  client$`{name}` <- function(input{default}, cancellation = NULL) client${dispatch}(\"{}\", input, cancellation)\n",
                operation.name()
            )
        };
        let call = method(&entry.method, "call");
        assert!(client.contains(&call), "{call}");
        assert_eq!(entry.paginators.is_some(), operation.paginated.is_some());
        for paginator in entry.paginators.iter().flatten() {
            let pages = method(paginator, "pages");
            assert!(client.contains(&pages), "{pages}");
        }
        for (structure, style, id) in entry
            .input
            .iter()
            .zip(&operation.input)
            .map(|(input, id)| (input, "keywords", id))
            .chain(
                entry
                    .output
                    .iter()
                    .zip(&operation.output)
                    .map(|(output, id)| (output, "elements", id)),
            )
        {
            assert_eq!(structure.style, style);
            assert!(exported(&structure.type_name), "{}", structure.type_name);
            // The constructor builds this operation's model, and is the
            // one place its arguments (the value's elements) are declared.
            let model = file(&files, &format!("R/model-{}.R", structure.type_name));
            let constructor = model
                .lines()
                .find(|line| line.starts_with(&format!("`{}` <- function(", structure.type_name)))
                .unwrap_or_else(|| panic!("constructor {}", structure.type_name));
            assert!(
                constructor.contains(&format!(".crucible_model(\"{id}\", ")),
                "{constructor}"
            );
            let parameters = &constructor[..constructor.find(") .crucible_model").unwrap()];
            for member in &structure.members {
                let parameter = if member.optional {
                    format!("`{}` = UNSET", member.name)
                } else {
                    format!("`{}`", member.name)
                };
                assert!(
                    parameters.contains(&format!("({parameter}"))
                        || parameters.contains(&format!(", {parameter}")),
                    "{constructor} takes {parameter}"
                );
                assert!(
                    constructor.contains(&format!("\"{0}\" = `{0}`", member.name)),
                    "{constructor} has {}",
                    member.name
                );
            }
        }
    }
    surface
}

#[test]
fn weather_surface_names_the_generated_declarations() {
    let service = load("weather");
    let surface = assert_declared(&service, &options());
    assert_eq!(surface.language, "r");
    assert_eq!(surface.package.ecosystem, "cran");
    assert_eq!(surface.package.name, "weather.client");
    assert_eq!(surface.client.name, "WeatherClient");
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
    assert!(revision.optional);
}

/// Another package name, with a generated-in endpoint: the client needs
/// no configuration.
#[test]
fn a_default_endpoint_needs_no_configuration() {
    let options = Options {
        package_name: "forecast".into(),
        package_version: "0.0.0".into(),
        endpoint: Some("https://api.weather.example".into()),
    };
    let surface = assert_declared(&load("weather"), &options);
    assert_eq!(surface.client.construct, "WeatherClient()");
    assert_eq!(surface.package.imports, ["library(forecast)"]);
    assert_eq!(
        surface.endpoint.as_deref(),
        Some("https://api.weather.example")
    );
}

/// Members named for R reserved words take the `_value` suffix the
/// constructor declares; the others keep their names.
#[test]
fn keyword_members_are_named_as_declared() {
    let surface = assert_declared(&load("client-docs-surface-keywords"), &options());
    let input = surface.operations["example.keywords#Describe"]
        .input
        .as_ref()
        .unwrap();
    let name = |member: &str| {
        input
            .members
            .iter()
            .find(|m| m.member == member)
            .unwrap()
            .name
            .clone()
    };
    assert_eq!(name("in"), "in_value");
    assert_eq!(name("class"), "class");
    assert_eq!(name("type"), "type");
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
