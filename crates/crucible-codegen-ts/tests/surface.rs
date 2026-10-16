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

/// The text from `start` to the end of its indented block.
fn block<'s>(source: &'s str, start: &str, end: &str) -> Option<&'s str> {
    let at = source.find(start)?;
    let rest = &source[at..];
    Some(&rest[..rest.find(end).unwrap_or(rest.len())])
}

/// Every name the surface gives is declared in the emitted package: each
/// method in `client.ts`, taking its operation's input interface and
/// sending that operation; each paginator calling its method; each
/// interface and its properties, with their types, in `models.ts`; the
/// imports and constructor in `index.ts`, `client.ts` and the runtime.
fn assert_declared(service: &crucible_ir::ir::Service, options: &Options, surface: &Surface) {
    let client = source(service, options, "src/client.ts");
    let models = source(service, options, "src/models.ts");
    let index = source(service, options, "src/index.ts");
    let runtime = source(service, options, "src/runtime.ts");
    let name = &surface.client.name;
    assert!(
        client.contains(&format!("export class {name} {{")),
        "{name}"
    );
    assert!(index.contains(&format!("export {{ {name} }}")));
    assert_eq!(
        surface.package.imports[0],
        format!("import {{ {name} }} from \"{}\";", options.package_name)
    );
    assert!(client.contains("constructor(config: ClientConfig = {})"));
    if surface.client.construct.contains("{ endpoint }") {
        assert!(options.endpoint.is_none());
        assert!(runtime.contains("readonly endpoint?: string;"));
    } else {
        assert_eq!(surface.client.construct, format!("new {name}()"));
        assert!(options.endpoint.is_some());
    }
    assert_eq!(surface.operations.len(), service.operations.len());
    let unqualified = |ty: &str| ty.replace(&surface.package.qualifier, "");
    for operation in &service.operations {
        let id = operation.id.to_string();
        let entry = &surface.operations[&id];
        let parameter = match &entry.input {
            Some(input) => format!("(input: m.{}", unqualified(&input.type_name)),
            None => "(options?: RequestOptions)".to_owned(),
        };
        let method = block(
            &client,
            &format!(
                "
  {}{parameter}",
                entry.method
            ),
            "
  }
",
        )
        .unwrap_or_else(|| panic!("{id}: {}{parameter}", entry.method));
        assert!(
            method.contains(&format!("this.#transport.send(spec.{},", operation.name())),
            "{id}: {} sends another operation",
            entry.method
        );
        for paginator in entry.paginators.iter().flatten() {
            let body = block(
                &client,
                &format!(
                    "
  {paginator}{parameter}"
                ),
                "
  }
",
            )
            .unwrap_or_else(|| panic!("{id}: paginator {paginator}"));
            assert!(
                body.contains(&format!("this.{}(page, options)", entry.method)),
                "{id}: {paginator} pages another method"
            );
        }
        let expected = [
            (&operation.input, &entry.input),
            (&operation.output, &entry.output),
        ];
        for (shape, structure) in expected {
            let (Some(shape), Some(structure)) = (shape, structure) else {
                assert_eq!(shape.is_some(), structure.is_some(), "{id}");
                continue;
            };
            assert_eq!(
                unqualified(&structure.type_name),
                service.name_of(shape),
                "{id}"
            );
            let body = block(
                &models,
                &format!("export interface {} {{", service.name_of(shape)),
                "
}",
            )
            .unwrap_or_else(|| panic!("{id}: interface {}", structure.type_name));
            for member in &structure.members {
                let declared = body
                    .lines()
                    .find_map(|line| {
                        let line = line.strip_prefix("  ")?;
                        let (key, ty) = line.split_once(": ")?;
                        (key.trim_end_matches('?') == member.name)
                            .then(|| (key.ends_with('?'), ty.trim_end_matches(';').to_owned()))
                    })
                    .unwrap_or_else(|| panic!("{id}: {}.{}", structure.type_name, member.name));
                assert_eq!(
                    unqualified(&member.type_name),
                    declared.1.replace("globalThis.", ""),
                    "{id}: {}.{}",
                    structure.type_name,
                    member.name
                );
                if structure.style == "properties" {
                    assert_eq!(member.optional, declared.0, "{id}: {}", member.name);
                }
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
    assert_eq!(input.type_name, "models.ListCitiesInput");
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
    for name in [
        "client-docs-surface-collisions",
        "client-docs-surface-keywords",
        "hardening",
        "extended-types",
    ] {
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
