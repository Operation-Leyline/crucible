//! The Cargo SDK's surface (`crucible.sdks/1`) names what the generated
//! client declares: methods, `*_with_options`, `*_pages`, model builders'
//! setters and accessors, with the suffixes the allocators assign.

use crucible_codegen_common::surface::{self, Surface};
use crucible_codegen_rust::{ClientOptions, generate_client, pages_method, surface};

mod common;

/// The `impl {name} {` block of `source`.
fn block<'s>(source: &'s str, name: &str) -> &'s str {
    let start = source
        .find(&format!(
            "impl {name} {{
"
        ))
        .unwrap_or_else(|| panic!("impl {name}"));
    let end = source[start..]
        .find(
            "
}
",
        )
        .unwrap();
    &source[start..start + end]
}

/// The method `name`'s declaration through the end of its body.
fn method<'s>(block: &'s str, name: &str) -> Option<&'s str> {
    let start = block
        .find(&format!("pub async fn {name}("))
        .or_else(|| block.find(&format!("pub fn {name}(")))?;
    let rest = &block[start..];
    Some(
        &rest[..rest
            .find(
                "
    }
",
            )
            .unwrap_or(rest.len())],
    )
}

/// The type the declaration `start` names, up to `end`, with paths
/// dropped: the generated code spells types by hidden absolute paths.
fn declared_type(block: &str, start: &str, end: &str) -> String {
    let at = block.find(start).unwrap_or_else(|| panic!("{start}")) + start.len();
    let ty = &block[at..at + block[at..].find(end).unwrap()];
    surface::unqualified(ty, "::")
}

/// Every name the surface gives is declared in the generated client: each
/// method taking its operation's input and sending that operation, each
/// builder setter and accessor with the member's type, and the client's
/// constructor taking the runtime's `Config`.
fn assert_declared(service: &crucible_ir::ir::Service, surface: &Surface) {
    let source = generate_client(service).unwrap();
    assert!(source.contains(&format!("pub struct {} {{", surface.client.name)));
    let client = block(&source, &surface.client.name);
    assert!(client.contains(
        "pub fn new(config: ::crucible_client_runtime_rust::__generated::Config) -> Self"
    ));
    assert_eq!(
        surface.client.construct,
        format!(
            "{}::new(runtime::Config::new(endpoint))",
            surface.client.name
        )
    );
    assert_eq!(surface.operations.len(), service.operations.len());
    for operation in &service.operations {
        let id = operation.id.to_string();
        let entry = &surface.operations[&id];
        let name = operation.name();
        let methods = std::iter::once(&entry.method).chain(&entry.options_method);
        for method_name in methods {
            let body = method(client, method_name).unwrap_or_else(|| panic!("{id}: {method_name}"));
            match &entry.input {
                Some(input) => assert!(
                    body.contains(&format!("input: {},", input.type_name))
                        || body.contains(&format!("input: {})", input.type_name)),
                    "{id}: {method_name} takes {}",
                    input.type_name
                ),
                None => assert!(!body.contains("input:"), "{id}: {method_name}"),
            }
            assert!(
                body.contains(&format!("::<{name}, {name}Error>")),
                "{id}: {method_name} sends another operation"
            );
        }
        for paginator in entry.paginators.iter().flatten() {
            let body = method(client, paginator).unwrap_or_else(|| panic!("{id}: {paginator}"));
            assert!(
                body.contains(&format!("Paginator<{name}, {name}Error>")),
                "{id}"
            );
        }
        if let Some(input) = &entry.input {
            assert_eq!(
                Some(input.type_name.as_str()),
                operation.input.as_ref().map(|id| service.name_of(id))
            );
            assert_eq!(input.style, "builder");
            let builder = block(&source, &format!("{}Builder", input.type_name));
            for member in &input.members {
                let start = format!(
                    "pub fn {}(mut self, value: impl ::std::convert::Into<",
                    member.name
                );
                assert_eq!(
                    declared_type(builder, &start, ">) -> Self"),
                    surface::unqualified(&member.type_name, "::"),
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
        if let Some(output) = &entry.output {
            assert_eq!(output.style, "methods");
            let model = block(&source, &output.type_name);
            for member in &output.members {
                let start = format!("pub fn {}(&self) -> ", member.name);
                let declared = declared_type(model, &start, " {");
                let declared = declared
                    .strip_prefix("Option<&")
                    .map(|ty| ty.strip_suffix('>').unwrap().to_owned())
                    .unwrap_or_else(|| declared.trim_start_matches('&').to_owned());
                assert_eq!(
                    declared,
                    surface::unqualified(&member.type_name, "::"),
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
    assert_eq!(surface.package.imports, ["use weather_sdk::*;"]);
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(
        surface.client.construct,
        "WeatherClient::new(runtime::Config::new(endpoint))"
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

/// Recursive models, keyword members (raw identifiers or renamed, as the
/// builder declares them) and every simple type match the emitted client.
#[test]
fn boxed_and_keyword_members_match_the_emitted_client() {
    for name in [
        "recursion",
        "client-docs-surface-keywords",
        "extended-types",
    ] {
        let service = common::load(name);
        assert_declared(&service, &surface(&service, &ClientOptions::default()));
    }
    let service = common::load("client-docs-surface-keywords");
    let surface = surface(&service, &ClientOptions::default());
    let input = surface.operations["example.keywords#Describe"]
        .input
        .as_ref()
        .unwrap();
    let names: Vec<_> = input
        .members
        .iter()
        .map(|member| member.name.as_str())
        .collect();
    assert!(names.contains(&"r#type"), "{names:?}");
    assert!(input.members.iter().all(|member| member.optional));
}
