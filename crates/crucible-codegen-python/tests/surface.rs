//! The surface names what the generated package declares.

use std::path::Path;

use crucible_codegen_common::surface::Surface;
use crucible_codegen_python::{Options, emit, surface};

fn load(name: &str) -> crucible_ir::ir::Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&dir).unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

fn options(endpoint: Option<&str>) -> Options {
    Options {
        package_name: "weather-client".into(),
        module_name: "weather_client".into(),
        package_version: "0.0.0.dev0".into(),
        endpoint: endpoint.map(str::to_owned),
    }
}

fn source(files: &[crucible_codegen_python::File], suffix: &str) -> String {
    files
        .iter()
        .find(|file| file.path.ends_with(suffix))
        .unwrap_or_else(|| panic!("no {suffix}"))
        .contents
        .clone()
}

/// The text of `class {name}` up to the next top-level declaration.
fn class_block<'a>(code: &'a str, name: &str) -> &'a str {
    let start = [format!("\nclass {name}:"), format!("\nclass {name}(")]
        .iter()
        .find_map(|header| code.find(header.as_str()))
        .unwrap_or_else(|| panic!("no class {name}"));
    let rest = &code[start + 1..];
    let end = rest[1..]
        .find("\nclass ")
        .or_else(|| rest[1..].find("\n@dataclass"))
        .map_or(rest.len(), |at| at + 1);
    &rest[..end]
}

/// Every name the surface gives is declared where it should be: each
/// method on the client, taking its operation's input type and calling its
/// own operation, and each member inside its own dataclass.
fn assert_declared(service: &crucible_ir::ir::Service, options: &Options, surface: &Surface) {
    let files = emit(service, options).unwrap();
    let client = source(&files, "client.py");
    let models = source(&files, "models.py");
    let init = source(&files, "__init__.py");
    let qualifier = &surface.package.qualifier;
    assert_eq!(qualifier, &format!("{}.", options.module_name));
    assert_eq!(
        surface.package.imports[0],
        format!("import {}", options.module_name)
    );
    let class = surface
        .client
        .name
        .strip_prefix(qualifier.as_str())
        .unwrap();
    let block = class_block(&client, class);
    assert!(block.starts_with(&format!("class {class}(SyncClient):")));
    assert!(init.contains(&format!("from .client import {class}, ")));
    // The constructor takes the endpoint keyword the construct passes.
    assert!(block.contains("def __init__(self, endpoint: str | None = None, **kwargs: Any)"));
    assert_eq!(surface.operations.len(), service.operations.len());
    for (id, operation) in &surface.operations {
        let op = service
            .operations
            .iter()
            .find(|op| op.id.to_string() == *id)
            .unwrap();
        let input = operation.input.as_ref().map(|input| {
            format!(
                "m.{}",
                input.type_name.strip_prefix(qualifier.as_str()).unwrap()
            )
        });
        let declared = |method: &str, call: &str| {
            let header = format!("    def {method}(self, ");
            let start = block
                .find(&header)
                .unwrap_or_else(|| panic!("{id}: no {method}"));
            let body = &block[start..];
            let body = &body[..body[1..].find("\n    def ").map_or(body.len(), |at| at + 1)];
            match &input {
                Some(input) => assert!(
                    body.starts_with(&format!("{header}input: {input}")),
                    "{id}: {method} takes {input}"
                ),
                None => assert!(
                    body.starts_with(&format!("{header}**options")),
                    "{id}: {method} takes no input"
                ),
            }
            assert!(
                body.contains(&format!("self.{call}(\"{}\", ", op.name())),
                "{id}: {method} calls {}",
                op.name()
            );
        };
        declared(&operation.method, "_call");
        for pages in operation.paginators.iter().flatten() {
            declared(pages, "_pages");
        }
        for structure in [&operation.input, &operation.output].into_iter().flatten() {
            let name = structure
                .type_name
                .strip_prefix(qualifier.as_str())
                .unwrap();
            assert!(
                init.contains(&format!("    {name},\n")),
                "{name} is exported"
            );
            let block = class_block(&models, name);
            for member in &structure.members {
                let native = member
                    .type_name
                    .strip_prefix(qualifier.as_str())
                    .unwrap_or(&member.type_name);
                let start = block
                    .find(&format!("\n    {}: {native}", member.name))
                    .unwrap_or_else(|| panic!("{name}.{}: {native}", member.name));
                let line = block[start + 1..].lines().next().unwrap();
                // An omittable member defaults to `UNSET`.
                assert_eq!(
                    member.optional,
                    line.contains("= field(default=UNSET"),
                    "{name}.{}: {line}",
                    member.name
                );
            }
        }
    }
}

#[test]
fn surface_names_what_the_package_declares() {
    for model in [
        "weather",
        "client-docs",
        "client-docs-surface-keywords",
        "client-docs-surface-defaults",
        "extended-types",
    ] {
        let service = load(model);
        for endpoint in [Some("https://weather.example"), None] {
            let options = options(endpoint);
            let surface = surface(&service, &options);
            assert_eq!(surface.language, "python");
            assert_eq!(surface.package.ecosystem, "pypi");
            assert_eq!(surface.package.name, "weather-client");
            assert_eq!(surface.package.import, "weather_client");
            assert_declared(&service, &options, &surface);
        }
    }
}

#[test]
fn weather_surface_spells_python() {
    let service = load("weather");
    let surface = surface(&service, &options(None));
    assert_eq!(surface.client.name, "weather_client.WeatherClient");
    assert_eq!(
        surface.client.construct,
        "weather_client.WeatherClient(endpoint=endpoint)"
    );
    assert!(!surface.client.statements && !surface.client.fallible);
    let configured =
        crucible_codegen_python::surface(&service, &options(Some("https://x.example")));
    assert_eq!(
        configured.client.construct,
        "weather_client.WeatherClient()"
    );
    assert_eq!(
        surface.package.imports,
        ["import weather_client"],
        "Weather's members name only its own aliases"
    );
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "list_cities");
    assert_eq!(list.paginators, Some(vec!["list_cities_pages".to_owned()]));
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, "weather_client.ListCitiesInput");
    assert_eq!(input.style, "keywords");
    assert_eq!(input.members[0].name, "next_token");
    assert_eq!(input.members[0].type_name, "str");
    assert!(input.members[0].optional);
    assert_eq!(list.output.as_ref().unwrap().style, "properties");
    let create = surface.operations["example.weather#CreateCity"]
        .input
        .as_ref()
        .unwrap();
    assert_eq!(create.members[0].type_name, "weather_client.CityName");
    assert!(!create.members[0].optional);
    assert!(
        surface.operations["example.weather#Ping"]
            .paginators
            .is_none()
    );
}

/// Reserved words take the emitter's `_` suffix.
#[test]
fn keywords_take_the_emitted_suffix() {
    let surface = surface(&load("client-docs-surface-keywords"), &options(None));
    let input = surface.operations["example.keywords#Describe"]
        .input
        .as_ref()
        .unwrap();
    let names: Vec<_> = input.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "in_", "object", "type", "default", "self_", "class_", "fun", "def_", "func", "end",
            "when", "struct"
        ]
    );
}

/// Python refuses method collisions instead of suffixing them, so there is
/// never a surface for a package that does not generate.
#[test]
fn colliding_method_names_are_refused() {
    let service = load("client-docs-surface-collisions");
    assert!(emit(&service, &options(None)).is_err());
}

/// An input member with a modeled default may be left out (the server
/// applies it); the same member of an output is always present.
#[test]
fn defaulted_input_members_are_omittable() {
    let service = load("client-docs-surface-defaults");
    let surface = surface(&service, &options(None));
    let operation = &surface.operations["example.defaults#Configure"];
    let optional = |members: &[crucible_codegen_common::surface::Member]| {
        members
            .iter()
            .map(|member| (member.member.clone(), member.optional))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        optional(&operation.input.as_ref().unwrap().members),
        [
            ("name".to_owned(), false),
            ("count".to_owned(), true),
            ("mode".to_owned(), true),
            ("token".to_owned(), true),
            ("note".to_owned(), true),
        ]
    );
    assert_eq!(
        optional(&operation.output.as_ref().unwrap().members),
        [
            ("name".to_owned(), false),
            ("count".to_owned(), false),
            ("note".to_owned(), true),
        ]
    );
}

/// Each snippet is a program over the surface's own names: the package
/// import, the client class, the operation's method and input dataclass,
/// and, for a paginated operation, the paginator.
#[test]
fn snippets_call_the_declared_names() {
    for model in ["weather", "samples", "client-docs", "extended-types"] {
        let service = load(model);
        for endpoint in [Some("https://weather.example"), None] {
            let options = options(endpoint);
            let surface = surface(&service, &options);
            for (id, operation) in &surface.operations {
                let snippet = &operation.snippet;
                assert!(
                    snippet
                        .lines()
                        .any(|line| line == format!("import {}", options.module_name)),
                    "{id}: {snippet}"
                );
                assert!(
                    snippet.contains(&format!("client = {}(", surface.client.name)),
                    "{id}: {snippet}"
                );
                assert!(
                    snippet.contains(&format!("client.{}(", operation.method)),
                    "{id}: {snippet}"
                );
                assert_eq!(
                    snippet.contains("endpoint=os.environ["),
                    endpoint.is_none(),
                    "{id}: {snippet}"
                );
                if let Some(input) = &operation.input {
                    assert!(
                        snippet.contains(&format!("request = {}(", input.type_name)),
                        "{id}: {snippet}"
                    );
                }
                match (&operation.paginators, &operation.paginated_snippet) {
                    (Some(paginators), Some(paginated)) => assert!(
                        paginated.contains(&format!("for page in client.{}(", paginators[0])),
                        "{id}: {paginated}"
                    ),
                    (None, None) => {}
                    _ => panic!("{id}: a paginated snippet exactly when there is a paginator"),
                }
            }
            // Generation is deterministic.
            let again = surface_snippets(&crucible_codegen_python::surface(&service, &options));
            assert_eq!(surface_snippets(&surface), again);
        }
    }
}

fn surface_snippets(surface: &Surface) -> Vec<(String, Option<String>)> {
    surface
        .operations
        .values()
        .map(|op| (op.snippet.clone(), op.paginated_snippet.clone()))
        .collect()
}

/// The weather `GetCity` snippet in full: the example's values, the
/// modelled error and its status.
#[test]
fn weather_get_city_snippet() {
    let surface = surface(&load("weather"), &options(Some("https://weather.example")));
    let snippet = &surface.operations["example.weather#GetCity"].snippet;
    assert!(snippet.starts_with(
        "import os\nfrom datetime import datetime, timezone\n\nimport weather_client\n"
    ));
    assert!(
        snippet.contains(
            "client = weather_client.WeatherClient(token=os.environ[\"WEATHER_TOKEN\"])\n"
        )
    );
    assert!(snippet.contains(
        "    city_id=\"c-0a1b2c3d\",\n    at=datetime(2026, 1, 2, 3, 4, 5, tzinfo=timezone.utc),\n"
    ));
    assert!(snippet.contains("except weather_client.NotFoundError as error:\n    print(f\"NotFoundError (HTTP {error.metadata.status}): {error.message}\")\n"));
}
