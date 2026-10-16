//! The surface names what the generated package declares.

use crucible_codegen_common::surface::Surface;
use crucible_codegen_php::{File, Options, emit, surface};
use crucible_ir::ir::Service;
use std::path::{Path, PathBuf};

fn model_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn load(name: &str) -> Service {
    crucible_lang::load_service(&model_dir(name)).unwrap()
}

fn options(endpoint: Option<&str>) -> Options {
    Options {
        package_name: "example/weather-client".into(),
        namespace: r"Example\Weather".into(),
        package_version: "0.0.0".into(),
        endpoint: endpoint.map(str::to_owned),
    }
}

fn file<'a>(files: &'a [File], path: &str) -> &'a str {
    &files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("no {path}"))
        .contents
}

/// Every name the surface gives is declared where it says: each method and
/// paginator on the client, taking its operation's input and calling that
/// operation; each member as a promoted readonly property of its own model
/// class, documented with the stated type, defaulting to absent exactly
/// when the member is optional.
fn assert_declared(service: &Service, options: &Options, surface: &Surface) {
    let files = emit(service, options).unwrap();
    let qualifier = &surface.package.qualifier;
    let client_name = surface
        .client
        .name
        .strip_prefix(qualifier.as_str())
        .unwrap();
    let client = file(&files, &format!("src/{client_name}.php"));
    assert!(client.contains(&format!("namespace {};", options.namespace)));
    assert!(client.contains(&format!("final class {client_name}\n")));
    assert_eq!(surface.operations.len(), service.operations.len());
    for op in &service.operations {
        let operation = &surface.operations[&op.id.to_string()];
        let input = operation
            .input
            .as_ref()
            .map(|input| format!("{} $input, ", input.type_name))
            .unwrap_or_default();
        let argument = if operation.input.is_some() {
            "$input"
        } else {
            r"new Runtime\Unit()"
        };
        let method = format!(
            "    public function {}({input}?Runtime\\CancellationToken $cancellation = null): ",
            operation.method
        );
        let start = client
            .find(&method)
            .unwrap_or_else(|| panic!("{}: {method}", op.id));
        let call = format!(
            "return $this->runtime->call('{}', {argument}, $cancellation);",
            op.name()
        );
        let body = &client[start..];
        let body = &body[..body.find("\n    }\n").unwrap()];
        assert!(body.contains(&call), "{}: {call}", op.id);
        for paginator in operation.paginators.iter().flatten() {
            let method = format!(
                "    public function {paginator}({input}?Runtime\\CancellationToken $cancellation = null): \\Generator\n    {{\n        return $this->runtime->pages('{}', {argument}, $cancellation);",
                op.name()
            );
            assert!(client.contains(&method), "{}: {method}", op.id);
        }
        for (structure, style) in [
            (&operation.input, "keywords"),
            (&operation.output, "properties"),
        ] {
            let Some(structure) = structure else { continue };
            assert_eq!(structure.style, style);
            let name = structure
                .type_name
                .strip_prefix(qualifier.as_str())
                .unwrap_or_else(|| panic!("{} is qualified", structure.type_name));
            let model = file(&files, &format!("src/{name}.php"));
            assert!(model.contains(&format!("final class {name} ")), "{name}");
            for member in &structure.members {
                let field = &member.name;
                // A promoted, readonly constructor parameter: a named
                // argument and a property at once.
                let promoted = model
                    .lines()
                    .map(str::trim)
                    .find(|line| {
                        line.starts_with("public readonly ")
                            && (line.contains(&format!(" ${field} "))
                                || line.ends_with(&format!(" ${field},")))
                    })
                    .unwrap_or_else(|| panic!("{name}::${field} in\n{model}"));
                assert_eq!(
                    promoted.contains("Absent::Value"),
                    member.optional,
                    "{name}::${field} optional: {promoted}"
                );
                // The docblock names the type the surface reports.
                let documented = model.lines().map(str::trim).any(|line| {
                    line.starts_with(&format!("* @param {}", member.type_name))
                        && line.ends_with(&format!(" ${field}"))
                });
                assert!(
                    documented,
                    "{name}::${field}: {} in\n{model}",
                    member.type_name
                );
            }
        }
    }
}

#[test]
fn surface_names_what_the_package_declares() {
    let service = load("weather");
    let options = options(Some("https://weather.example"));
    let surface = surface(&service, &options);

    assert_eq!(surface.language, "php");
    assert_eq!(surface.package.ecosystem, "composer");
    assert_eq!(surface.package.name, "example/weather-client");
    assert_eq!(surface.package.import, r"Example\Weather");
    // Composer's autoloader loads every class; names are fully qualified.
    assert!(surface.package.imports.is_empty());
    assert_eq!(surface.package.qualifier, r"\Example\Weather\");
    assert_eq!(surface.client.name, r"\Example\Weather\WeatherClient");
    assert_eq!(
        surface.client.construct,
        r"new \Example\Weather\WeatherClient()"
    );
    assert!(!surface.client.statements);
    assert!(!surface.client.fallible);
    assert_eq!(surface.endpoint.as_deref(), Some("https://weather.example"));

    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "listCities");
    assert_eq!(list.paginators, Some(vec!["listCitiesPages".to_owned()]));
    assert_eq!(
        surface.operations["example.weather#GetCity"].paginators,
        None
    );
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, r"\Example\Weather\ListCitiesInput");
    let tags = input.members.iter().find(|m| m.member == "tags").unwrap();
    assert_eq!(
        (tags.name.as_str(), tags.type_name.as_str(), tags.optional),
        ("tags", "list<string>", true)
    );
    // Runtime value classes keep their namespace, so they resolve.
    let summary = surface.operations["example.weather#CreateCity"]
        .output
        .as_ref()
        .unwrap();
    assert_eq!(
        summary.members[0].type_name,
        r"\Example\Weather\CitySummary"
    );
    let output = surface.operations["example.weather#GetCity"]
        .output
        .as_ref()
        .unwrap();
    let forecast = output
        .members
        .iter()
        .find(|m| m.member == "forecast")
        .unwrap();
    assert_eq!(
        forecast.type_name,
        r"array<string|int, \Example\Weather\Runtime\JsonValue>"
    );
    assert_declared(&service, &options, &surface);
}

#[test]
fn construction_reads_an_endpoint_when_none_is_generated() {
    let service = load("weather");
    let options = options(None);
    let surface = surface(&service, &options);
    assert_eq!(
        surface.client.construct,
        r"new \Example\Weather\WeatherClient(new \Example\Weather\Runtime\ClientConfig(endpoint: $endpoint))"
    );
    // The client takes a `Runtime\ClientConfig`, whose `endpoint` is a
    // named constructor parameter.
    let files = emit(&service, &options).unwrap();
    let client = file(&files, "src/WeatherClient.php");
    assert!(client.contains(
        r"public function __construct(Runtime\ClientConfig $config = new Runtime\ClientConfig())"
    ));
    let config = file(&files, "src/Runtime/ClientConfig.php");
    assert!(config.contains("public ?string $endpoint = null"));
    assert_declared(&service, &options, &surface);
}

/// Another namespace qualifies every name with it.
#[test]
fn the_namespace_qualifies_every_name() {
    let service = load("weather");
    let options = Options {
        package_name: "acme/forecasts".into(),
        namespace: r"Acme\Forecasts".into(),
        ..options(None)
    };
    let surface = surface(&service, &options);
    assert_eq!(surface.package.qualifier, r"\Acme\Forecasts\");
    assert_eq!(surface.client.name, r"\Acme\Forecasts\WeatherClient");
    let create = surface.operations["example.weather#CreateCity"]
        .input
        .as_ref()
        .unwrap();
    assert_eq!(create.type_name, r"\Acme\Forecasts\CreateCityInput");
    let location = create
        .members
        .iter()
        .find(|m| m.member == "location")
        .unwrap();
    assert_eq!(location.type_name, r"\Acme\Forecasts\Location");
    assert_declared(&service, &options, &surface);
}

/// Reserved words are named as the models declare them.
#[test]
fn keyword_members_are_named_as_declared() {
    let service = load("client-docs-surface-keywords");
    let options = options(None);
    let surface = surface(&service, &options);
    assert_declared(&service, &options, &surface);
}

#[test]
fn derived_name_collisions_are_refused_not_suffixed() {
    let service = load("client-docs-surface-collisions");
    let diagnostics = match emit(&service, &options(None)) {
        Ok(_) => panic!("ListThingsPages collides with listThings' Pages generator"),
        Err(diagnostics) => diagnostics.to_string(),
    };
    assert!(
        diagnostics.contains("PHP operation name collision"),
        "{diagnostics}"
    );
}

/// A member with a modeled default is no more omittable than a required
/// one: the generated constructor (or encoder) requires both, and only
/// `optional_on_client` members may be left out, as `optional` says.
#[test]
fn defaulted_members_are_as_optional_as_the_emitted_code_says() {
    let service = load("client-docs-surface-defaults");
    let surface = {
        let options = options(None);
        let surface = surface(&service, &options);
        assert_declared(&service, &options, &surface);
        surface
    };
    let configure = &surface.operations["example.defaults#Configure"];
    fn optional(structure: &crucible_codegen_common::surface::Structure) -> Vec<(&str, bool)> {
        structure
            .members
            .iter()
            .map(|member| (member.member.as_str(), member.optional))
            .collect()
    }
    assert_eq!(
        optional(configure.input.as_ref().unwrap()),
        [
            ("name", false),
            ("count", false),
            ("mode", false),
            ("token", true),
            ("note", true)
        ]
    );
    assert_eq!(
        optional(configure.output.as_ref().unwrap()),
        [("name", false), ("count", false), ("note", true)]
    );
}

/// Each snippet is a script calling its own operation through the declared
/// client, building the declared input class, and a paginated snippet
/// iterates the declared paginator.
#[test]
fn snippets_call_what_the_package_declares() {
    for (model, endpoint) in [
        ("weather", None),
        ("weather", Some("https://weather.example")),
        ("samples", None),
    ] {
        let service = load(model);
        let options = options(endpoint);
        let surface = surface(&service, &options);
        for (id, operation) in &surface.operations {
            let snippet = &operation.snippet;
            assert!(
                snippet.starts_with("<?php\ndeclare(strict_types=1);\n"),
                "{id}"
            );
            assert!(
                snippet.contains(&format!("new {}(", surface.client.name)),
                "{id}: {snippet}"
            );
            assert!(
                snippet.contains(&format!("$client->{}(", operation.method)),
                "{id}: {snippet}"
            );
            if let Some(input) = &operation.input {
                assert!(
                    snippet.contains(&format!("$input = new {}(", input.type_name)),
                    "{id}: {snippet}"
                );
            }
            let paginator = operation.paginators.iter().flatten().next();
            assert_eq!(
                paginator.is_some(),
                operation.paginated_snippet.is_some(),
                "{id}"
            );
            if let (Some(paginator), Some(paged)) = (paginator, &operation.paginated_snippet) {
                assert!(
                    paged.contains(&format!("foreach ($client->{paginator}(")),
                    "{id}: {paged}"
                );
            }
        }
        if endpoint.is_none() {
            assert!(
                surface
                    .operations
                    .values()
                    .all(|op| op.snippet.contains("endpoint: getenv('")),
                "{model}: the endpoint is read when none is generated"
            );
        }
    }
}
