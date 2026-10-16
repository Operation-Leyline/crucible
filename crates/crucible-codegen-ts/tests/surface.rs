//! The client surface (`crucible.sdks/1`) names what the emitted client
//! declares: methods, paginators, interfaces and their properties, with
//! the collision suffixes the allocator assigns.

use std::path::Path;

use crucible_codegen_common::surface::Surface;
use crucible_codegen_ts::{Middleware, Options, emit, surface};

fn load(name: &str) -> crucible_ir::ir::Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&dir).unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

fn options() -> Options {
    Options {
        package_name: "@example/weather".into(),
        package_version: "0.0.0".into(),
        client_name: None,
        endpoint: None,
        middleware: vec![],
        package_json: None,
        metadata: Default::default(),
    }
}

/// The emitted file at `path`.
fn source(service: &crucible_ir::ir::Service, options: &Options, path: &str) -> String {
    emit(service, options)
        .unwrap()
        .into_iter()
        .find(|file| file.path == path)
        .unwrap()
        .contents
}

/// Every name the surface gives is declared in the emitted client and
/// models: a method or paginator in `client.ts`, an interface and each of
/// its properties in `models.ts`.
fn assert_declared(service: &crucible_ir::ir::Service, options: &Options, surface: &Surface) {
    let client = source(service, options, "src/client.ts");
    let models = source(service, options, "src/models.ts");
    let index = source(service, options, "src/index.ts");
    assert!(
        client.contains(&format!("export class {} {{", surface.client.name)),
        "{}",
        surface.client.name
    );
    assert!(index.contains(&format!("export {{ {} }}", surface.client.name)));
    assert_eq!(surface.operations.len(), service.operations.len());
    for (id, operation) in &surface.operations {
        assert!(
            client.contains(&format!("\n  {}(", operation.method)),
            "{id}: method {}",
            operation.method
        );
        for paginator in operation.paginators.iter().flatten() {
            assert!(
                client.contains(&format!("\n  {paginator}(")),
                "{id}: paginator {paginator}"
            );
        }
        for structure in operation.input.iter().chain(&operation.output) {
            let start = models
                .find(&format!("export interface {} {{", structure.type_name))
                .unwrap_or_else(|| panic!("{id}: interface {}", structure.type_name));
            let body = &models[start..start + models[start..].find("\n}").unwrap()];
            for member in &structure.members {
                assert!(
                    body.contains(&format!("\n  {}?: {}", member.name, member.type_name))
                        || body.contains(&format!("\n  {}: {}", member.name, member.type_name)),
                    "{id}: {}.{} as {}",
                    structure.type_name,
                    member.name,
                    member.type_name
                );
            }
        }
    }
}

#[test]
fn weather_surface_names_what_the_client_declares() {
    let service = load("weather");
    let options = options();
    let surface = surface(&service, &options);
    assert_eq!(surface.language, "ts");
    assert_eq!(surface.package.ecosystem, "npm");
    assert_eq!(surface.package.name, "@example/weather");
    assert_eq!(surface.package.import, "@example/weather");
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(surface.client.construct, "new WeatherClient({ endpoint })");
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "listCities");
    assert_eq!(
        list.paginators.as_deref(),
        Some(&["paginateListCities".to_owned()][..])
    );
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, "ListCitiesInput");
    assert_eq!(input.style, "object");
    let create = &surface.operations["example.weather#CreateCity"];
    let revision = create.input.as_ref().unwrap().members.iter();
    let revision = revision
        .into_iter()
        .find(|member| member.member == "expectedRevision")
        .unwrap();
    assert_eq!(revision.type_name, "bigint");
    assert!(
        surface.operations["example.weather#GetCity"]
            .paginators
            .is_none()
    );
    assert_declared(&service, &options, &surface);
}

#[test]
fn defaults_and_middleware_are_in_the_surface() {
    let service = load("weather");
    let options = Options {
        client_name: Some("Forecasts".into()),
        endpoint: Some("https://api.weather.example".into()),
        middleware: vec![Middleware::ContentSha256],
        ..options()
    };
    let surface = surface(&service, &options);
    assert_eq!(surface.client.name, "Forecasts");
    assert_eq!(surface.client.construct, "new Forecasts()");
    assert_eq!(
        surface.endpoint.as_deref(),
        Some("https://api.weather.example")
    );
    assert_eq!(surface.middleware.len(), 1);
    assert_eq!(surface.middleware[0].name, "content-sha256");
    assert_eq!(surface.middleware[0].headers, ["x-amz-content-sha256"]);
    assert_declared(&service, &options, &surface);
}

/// Names the allocator suffixes: `constructor` is the class's, and
/// `PaginateListThings` keeps `paginateListThings`, so `ListThings`'s
/// paginator takes `_2`.
#[test]
fn collision_suffixes_match_the_emitted_client() {
    for name in ["client-docs-surface-collisions", "hardening"] {
        let service = load(name);
        let options = options();
        let surface = surface(&service, &options);
        assert_declared(&service, &options, &surface);
    }
    let surface = surface(&load("client-docs-surface-collisions"), &options());
    let method = |id: &str| surface.operations[id].method.clone();
    assert_eq!(method("example.surface#Constructor"), "constructor_2");
    assert_eq!(
        method("example.surface#PaginateListThings"),
        "paginateListThings"
    );
    assert_eq!(
        surface.operations["example.surface#ListThings"]
            .paginators
            .as_deref(),
        Some(&["paginateListThings_2".to_owned()][..])
    );
    let output = surface.operations["example.surface#ListThings"]
        .output
        .as_ref()
        .unwrap();
    assert_eq!(output.members[2].type_name, "bigint");
}
