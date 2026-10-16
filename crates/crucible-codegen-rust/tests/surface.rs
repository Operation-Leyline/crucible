//! The Cargo SDK's surface (`crucible.sdks/1`) names what the generated
//! client declares: methods, `*_with_options`, `*_pages`, model builders'
//! setters and accessors, with the suffixes the allocators assign.

use crucible_codegen_common::surface::Surface;
use crucible_codegen_rust::{ClientOptions, generate_client, pages_method, surface};

mod common;

/// The `impl {name} {` block of `source`.
fn block<'s>(source: &'s str, name: &str) -> &'s str {
    let start = source
        .find(&format!("impl {name} {{\n"))
        .unwrap_or_else(|| panic!("impl {name}"));
    let end = source[start..].find("\n}\n").unwrap();
    &source[start..start + end]
}

fn assert_declared(service: &crucible_ir::ir::Service, surface: &Surface) {
    let source = generate_client(service).unwrap();
    assert!(source.contains(&format!("pub struct {} {{", surface.client.name)));
    let client = block(&source, &surface.client.name);
    assert_eq!(surface.operations.len(), service.operations.len());
    for (id, operation) in &surface.operations {
        let methods = std::iter::once(&operation.method)
            .chain(&operation.options_method)
            .chain(operation.paginators.iter().flatten());
        for method in methods {
            assert!(
                client.contains(&format!("pub async fn {method}("))
                    || client.contains(&format!("pub fn {method}(")),
                "{id}: {method}"
            );
        }
        if let Some(input) = &operation.input {
            assert_eq!(input.style, "builder");
            let builder = block(&source, &format!("{}Builder", input.type_name));
            for member in &input.members {
                assert!(
                    builder.contains(&format!("pub fn {}(mut self, value:", member.name)),
                    "{id}: setter {}",
                    member.name
                );
            }
            let build = if input.fallible {
                format!(
                    "pub fn build(self) -> ::std::result::Result<{}",
                    input.type_name
                )
            } else {
                format!("pub fn build(self) -> {} {{", input.type_name)
            };
            assert!(builder.contains(&build), "{id}: {build}");
        }
        if let Some(output) = &operation.output {
            assert_eq!(output.style, "methods");
            let model = block(&source, &output.type_name);
            for member in &output.members {
                assert!(
                    model.contains(&format!("pub fn {}(&self)", member.name)),
                    "{id}: accessor {}",
                    member.name
                );
            }
        }
    }
}

#[test]
fn weather_surface_names_what_the_client_declares() {
    let service = common::load("weather");
    let surface = surface(&service, &ClientOptions::default());
    assert_eq!(surface.language, "rust");
    assert_eq!(surface.package.ecosystem, "cargo");
    assert_eq!(surface.package.name, "weather-sdk");
    assert_eq!(surface.package.import, "weather_sdk");
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(
        surface.client.construct,
        "WeatherClient::new(Config::new(endpoint))"
    );
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "list_cities");
    assert_eq!(
        list.options_method.as_deref(),
        Some("list_cities_with_options")
    );
    assert_eq!(
        list.paginators.as_deref(),
        Some(&["list_cities_pages".to_owned()][..])
    );
    let create = surface.operations["example.weather#CreateCity"]
        .input
        .as_ref()
        .unwrap();
    assert!(create.fallible, "CreateCity requires a name");
    let revision = create
        .members
        .iter()
        .find(|member| member.member == "expectedRevision")
        .unwrap();
    assert_eq!(
        (revision.name.as_str(), revision.type_name.as_str()),
        ("expected_revision", "i64")
    );
    assert!(
        !surface.operations["example.weather#ListCities"]
            .input
            .as_ref()
            .unwrap()
            .fallible
    );
    assert_declared(&service, &surface);
    let named = ClientOptions {
        package_name: Some("leyline-sdk".into()),
        ..ClientOptions::default()
    };
    let surface = crucible_codegen_rust::surface(&service, &named);
    assert_eq!(surface.package.name, "leyline-sdk");
    assert_eq!(surface.package.import, "leyline_sdk");
}

/// `ListThingsPages` and `ListThingsWithOptions` keep their own names, so
/// `ListThings`'s derived methods take `_2`; builder members named `build`
/// and accessors named `info` take `member_`.
#[test]
fn collision_suffixes_match_the_emitted_client() {
    let service = common::load("client-docs-surface-collisions");
    let surface = surface(&service, &ClientOptions::default());
    assert_declared(&service, &surface);
    let list = &surface.operations["example.surface#ListThings"];
    assert_eq!(
        list.options_method.as_deref(),
        Some("list_things_with_options_2")
    );
    assert_eq!(
        list.paginators.as_deref(),
        Some(&["list_things_pages_2".to_owned()][..])
    );
    let op = service
        .operations
        .iter()
        .find(|op| op.id.to_string() == "example.surface#ListThings")
        .unwrap();
    assert_eq!(
        pages_method(&service, op).as_deref(),
        Some("list_things_pages_2")
    );
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.members[1].name, "member_build");
    let output = list.output.as_ref().unwrap();
    assert_eq!(output.members[1].name, "member_info");
    assert_eq!(output.members[2].type_name, "i64");
}
