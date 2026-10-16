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

/// A member with a modeled default is no more omittable than a required
/// one: the generated constructor (or encoder) requires both, and only
/// `optional_on_client` members may be left out, as `optional` says.
#[test]
fn defaulted_members_are_as_optional_as_the_emitted_code_says() {
    let service = load("client-docs-surface-defaults");
    let surface = assert_declared(&service, &options());
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

/// The surface's snippets call what the package declares: they attach the
/// package, build the exported client, construct the operation's input
/// with its exported constructor and call its method; a paginated
/// operation's second snippet pages with its page function. The package
/// ships both guides, `AGENTS.md` under `inst/`.
#[test]
fn snippets_call_the_declared_names() {
    for model in [
        "weather",
        "samples",
        "client-docs-surface-keywords",
        "client-docs-surface-defaults",
    ] {
        let service = load(model);
        let options = options();
        let surface = assert_declared(&service, &options);
        let files = emit(&service, &options).unwrap();
        let readme = file(&files, "README.md");
        let agents = file(&files, "inst/AGENTS.md");
        assert!(readme.starts_with("# weather.client\n"), "{model}");
        assert!(agents.contains("library(weather.client)"), "{model}");
        assert!(
            !files.iter().any(|f| f.path == "AGENTS.md"),
            "a top-level AGENTS.md is a check NOTE"
        );
        for (id, operation) in &surface.operations {
            let snippet = &operation.snippet;
            assert!(
                snippet.starts_with("library(weather.client)\n"),
                "{id}: {snippet}"
            );
            assert!(
                snippet.contains(&format!("client <- {}(", surface.client.name)),
                "{id}: {snippet}"
            );
            assert!(snippet.ends_with("client$close()\n"), "{id}: {snippet}");
            let argument = match &operation.input {
                Some(input) => {
                    assert!(
                        snippet.contains(&format!("input <- {}(", input.type_name)),
                        "{id}: {snippet}"
                    );
                    "input"
                }
                None => "",
            };
            assert!(
                snippet.contains(&format!("client${}({argument})", operation.method)),
                "{id}: {snippet}"
            );
            match (&operation.paginators, &operation.paginated_snippet) {
                (Some(pages), Some(paginated)) => assert!(
                    paginated.contains(&format!("next_page <- client${}(input)", pages[0])),
                    "{id}: {paginated}"
                ),
                (None, None) => {}
                other => panic!("{id}: {other:?}"),
            }
        }
    }
}

/// Weather's calls, spelled as the R package takes them.
#[test]
fn weather_snippets_spell_r_values() {
    let weather = surface(&load("weather"), &options());
    let get = &weather.operations["example.weather#GetCity"].snippet;
    assert!(get.contains("token = function() Sys.getenv(\"WEATHER_TOKEN\")"));
    assert!(get.contains("endpoint = Sys.getenv(\"WEATHER_ENDPOINT\")"));
    assert!(get.contains("# Values from the model's example \"Read a city\".\n"));
    assert!(get.contains("  at = timestamp(1767323045)\n"));
    assert!(get.contains("  NotFoundError = function(error) {\n"));
    assert!(get.contains("error$metadata$status"));
    let create = &weather.operations["example.weather#CreateCity"].snippet;
    assert!(
        create.contains("Location(\n    \"coordinates\",\n"),
        "{create}"
    );
    assert!(
        create.contains("expected_revision = long(\"7\")"),
        "{create}"
    );
    let pages = weather.operations["example.weather#ListCities"]
        .paginated_snippet
        .as_ref()
        .unwrap();
    assert!(!pages.contains("next_token ="), "{pages}");
    assert!(pages.contains("while (!is.null(page <- next_page())) {\n  print(page$cities)\n}\n"));
    // A generated-in endpoint is not read from the environment.
    let configured = Options {
        endpoint: Some("https://weather.example".into()),
        ..options()
    };
    let configured = surface(&load("weather"), &configured);
    assert!(
        !configured.operations["example.weather#GetCity"]
            .snippet
            .contains("WEATHER_ENDPOINT")
    );
}

/// An example that leaves out a member with a modeled default still
/// constructs: the R constructor requires the member, so the snippet
/// passes its default.
#[test]
fn an_example_without_defaulted_members_passes_their_defaults() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models/client-docs-surface-defaults");
    let mut package = crucible_lang::load_package(&dir).unwrap();
    let file = package
        .files
        .iter_mut()
        .find(|file| file.text.contains("operation Configure"))
        .unwrap();
    file.text = file.text.replace("\r\n", "\n").replacen(
        "    output {",
        "    example \"Name only\" {\n        input { name: \"n\" }\n    }\n    output {",
        1,
    );
    let compiled = crucible_lang::compile(&package, None);
    let service = compiled
        .service
        .clone()
        .unwrap_or_else(|| panic!("{}", compiled.render()));
    let surface = surface(&service, &options());
    let snippet = &surface.operations["example.defaults#Configure"].snippet;
    assert!(
        snippet.contains(
            "input <- ConfigureInput(\n  name = \"n\",\n  count = 17,\n  mode = \"fast\"\n)\n"
        ),
        "{snippet}"
    );
}
