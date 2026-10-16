//! The surface names what the generated module declares.

use std::path::Path;

use crucible_codegen_common::surface::Surface;
use crucible_codegen_go::{File, Options, emit, surface};

fn load(name: &str) -> crucible_ir::ir::Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&dir).unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

fn options(endpoint: Option<&str>) -> Options {
    Options {
        module_path: "example.com/weather".into(),
        package_name: "weather".into(),
        endpoint: endpoint.map(str::to_owned),
    }
}

fn source(files: &[File], path: &str) -> String {
    files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("no {path}"))
        .contents
        .clone()
}

/// The text from `header` to the end of its declaration (`\n}\n`).
fn block<'a>(code: &'a str, header: &str) -> &'a str {
    let start = code.find(header).unwrap_or_else(|| panic!("no {header}"));
    let rest = &code[start..];
    &rest[..rest.find("\n}\n").map_or(rest.len(), |at| at + 3)]
}

/// `native` as the module itself writes it, without `qualifier`.
fn local(native: &str, qualifier: &str) -> String {
    native.replace(qualifier, "")
}

/// Every name the surface gives is declared where it should be: each
/// method on the client, taking its operation's input and calling its own
/// operation; each member inside its own struct, with the field's type.
fn assert_declared(service: &crucible_ir::ir::Service, options: &Options, surface: &Surface) {
    let files = emit(service, options).unwrap();
    let client = source(&files, "client.go");
    let models = source(&files, "models.go");
    let runtime = source(&files, "runtime.go");
    let package = &options.package_name;
    let qualifier = &surface.package.qualifier;
    assert_eq!(qualifier, &format!("{package}."));
    assert_eq!(
        surface.package.imports[0],
        format!("import {package} \"{}\"", options.module_path)
    );
    assert!(
        client.starts_with(&format!("package {package}\n"))
            || client.contains(&format!("\npackage {package}\n"))
    );
    let name = surface
        .client
        .name
        .strip_prefix(qualifier.as_str())
        .unwrap();
    assert!(client.contains(&format!("type {name} struct ")));
    // The construct calls the emitted constructor with a `Config` whose
    // `Endpoint` field it sets, and handles its error result.
    assert!(client.contains(&format!(
        "func New{name}(config Config) (*{name}, error) {{"
    )));
    assert!(surface.client.fallible);
    assert!(
        surface
            .client
            .construct
            .starts_with(&format!("{package}.New{name}({package}.Config{{"))
    );
    if surface.client.construct.contains("Endpoint:") {
        assert!(block(&runtime, "type Config struct {").contains("\tEndpoint "));
    }
    assert_eq!(surface.operations.len(), service.operations.len());
    for (id, operation) in &surface.operations {
        let op = service
            .operations
            .iter()
            .find(|op| op.id.to_string() == *id)
            .unwrap();
        let input = operation.input.as_ref().map_or("Unit".to_owned(), |input| {
            format!("*{}", local(&input.type_name, qualifier))
        });
        let method = block(
            &client,
            &format!(
                "func (c *{name}) {}(ctx context.Context, input {input}) (",
                operation.method
            ),
        );
        assert!(
            method.contains(&format!("c.call(ctx, \"{}\", input)", op.name())),
            "{id}: {} calls {}",
            operation.method,
            op.name()
        );
        for pages in operation.paginators.iter().flatten() {
            let paginator = block(
                &client,
                &format!("func (c *{name}) {pages}(input {input}) (*"),
            );
            assert!(paginator.contains("encodeModel("), "{id}: {pages}");
            assert!(
                client.contains(&format!(
                    "p.client.callValues(ctx, \"{}\", p.input)",
                    op.name()
                )),
                "{id}: {pages} pages through {}",
                op.name()
            );
        }
        for structure in [&operation.input, &operation.output].into_iter().flatten() {
            let declared = block(
                &models,
                &format!(
                    "type {} struct {{\n",
                    local(&structure.type_name, qualifier)
                ),
            );
            for member in &structure.members {
                assert!(
                    declared.contains(&format!(
                        "\n    {} {}\n",
                        member.name,
                        local(&member.type_name, qualifier)
                    )),
                    "{}.{}: {}",
                    structure.type_name,
                    member.name,
                    member.type_name
                );
                assert_eq!(
                    member.optional,
                    member
                        .type_name
                        .starts_with(&format!("{qualifier}Optional[")),
                    "{}.{}",
                    structure.type_name,
                    member.name
                );
                for (uses, import) in [
                    ("time.", "import \"time\""),
                    ("big.", "import \"math/big\""),
                ] {
                    if member.type_name.contains(uses) {
                        assert!(surface.package.imports.iter().any(|line| line == import));
                    }
                }
            }
        }
    }
}

#[test]
fn surface_names_what_the_module_declares() {
    for model in [
        "weather",
        "client-docs",
        "client-docs-surface-keywords",
        "extended-types",
    ] {
        let service = load(model);
        for endpoint in [None, Some("https://weather.example")] {
            let options = options(endpoint);
            let surface = surface(&service, &options);
            assert_eq!(surface.language, "go");
            assert_eq!(surface.package.ecosystem, "go");
            assert_eq!(surface.package.name, "example.com/weather");
            assert_eq!(surface.package.import, "example.com/weather");
            assert_declared(&service, &options, &surface);
        }
    }
}

/// A package clause that differs from the import path's last element is
/// the alias the import and every qualified type use.
#[test]
fn the_package_name_qualifies_types() {
    let service = load("weather");
    let options = Options {
        module_path: "example.com/sdk/v2".into(),
        package_name: "forecast".into(),
        endpoint: None,
    };
    let surface = surface(&service, &options);
    assert_eq!(
        surface.package.imports[0],
        "import forecast \"example.com/sdk/v2\""
    );
    assert_eq!(surface.client.name, "forecast.WeatherClient");
    assert_eq!(
        surface.operations["example.weather#GetCity"]
            .input
            .as_ref()
            .unwrap()
            .type_name,
        "forecast.GetCityInput"
    );
    assert_declared(&service, &options, &surface);
}

#[test]
fn weather_surface_spells_go() {
    let service = load("weather");
    let surface = surface(&service, &options(None));
    assert_eq!(surface.client.name, "weather.WeatherClient");
    assert_eq!(
        surface.client.construct,
        "weather.NewWeatherClient(weather.Config{Endpoint: endpoint})"
    );
    assert!(surface.client.fallible && !surface.client.statements);
    assert_eq!(
        surface.package.imports,
        ["import weather \"example.com/weather\""]
    );
    let configured = crucible_codegen_go::surface(&service, &options(Some("https://x.example")));
    assert_eq!(
        configured.client.construct,
        "weather.NewWeatherClient(weather.Config{})"
    );
    assert_eq!(configured.endpoint.as_deref(), Some("https://x.example"));
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "ListCities");
    assert_eq!(list.paginators, Some(vec!["ListCitiesPages".to_owned()]));
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, "weather.ListCitiesInput");
    assert_eq!(input.style, "fields");
    assert_eq!(input.members[0].name, "NextToken");
    assert_eq!(input.members[0].type_name, "weather.Optional[string]");
    assert!(input.members[0].optional);
    let create = surface.operations["example.weather#CreateCity"]
        .input
        .as_ref()
        .unwrap();
    assert_eq!(create.members[0].type_name, "weather.CityName");
    assert_eq!(
        create.members[1].type_name,
        "weather.Optional[*weather.Location]"
    );
    assert!(
        surface.operations["example.weather#Ping"]
            .paginators
            .is_none()
    );
}

/// Go exports every member, so a reserved word becomes an ordinary
/// capitalized identifier.
#[test]
fn keywords_are_exported_identifiers() {
    let surface = surface(&load("client-docs-surface-keywords"), &options(None));
    let input = surface.operations["example.keywords#Describe"]
        .input
        .as_ref()
        .unwrap();
    let names: Vec<_> = input.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "In", "Object", "Type", "Default", "Self", "Class", "Fun", "Def", "Func", "End",
            "When", "Struct"
        ]
    );
}

/// Go refuses method collisions instead of suffixing them, so there is
/// never a surface for a module that does not generate.
#[test]
fn colliding_method_names_are_refused() {
    let service = load("client-docs-surface-collisions");
    assert!(emit(&service, &options(None)).is_err());
}
