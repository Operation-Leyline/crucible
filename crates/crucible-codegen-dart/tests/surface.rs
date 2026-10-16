//! The surface names what the generated package declares.

use crucible_codegen_common::surface::Surface;
use crucible_codegen_dart::{Options, emit, surface};
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
        package_name: "weather_client".into(),
        package_version: "0.0.0-dev".into(),
        endpoint: endpoint.map(str::to_owned),
    }
}

fn file<'a>(files: &'a [crucible_codegen_dart::File], path: &str) -> &'a str {
    &files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("no {path}"))
        .contents
}

/// The `final class {name} ...` declaration, up to its closing brace.
fn class_block<'a>(models: &'a str, name: &str) -> &'a str {
    let start = models
        .find(&format!("final class {name} "))
        .unwrap_or_else(|| panic!("final class {name}"));
    let end = models[start..].find("\n}\n").expect("class closes");
    &models[start..start + end]
}

/// A Dart type as the generated files write it (`core.String`, `stamp.Timestamp`),
/// as a caller writes it after the package import.
fn caller(ty: &str) -> String {
    crucible_codegen_common::surface::unqualified(ty, ".")
}

/// Every name the surface gives is declared where it says: each method and
/// paginator on the client, taking its operation's input and calling that
/// operation; each member as a field or getter of its own model class, of
/// the stated type, and as a constructor parameter that is required exactly
/// when the member is not optional.
fn assert_declared(service: &Service, options: &Options, surface: &Surface) {
    let files = emit(service, options).unwrap();
    let client = file(&files, "lib/src/client.dart");
    let models = file(&files, "lib/src/models.dart");
    assert!(client.contains(&format!("final class {} {{", surface.client.name)));
    assert_eq!(surface.operations.len(), service.operations.len());
    for op in &service.operations {
        let operation = &surface.operations[&op.id.to_string()];
        let call = format!("_runtime.call(\"{}\"", op.name());
        let parameter = match &operation.input {
            Some(input) => format!("(models.{} input, {{", input.type_name),
            None => "({CancellationToken? cancellation})".to_owned(),
        };
        let declared = client.lines().any(|line| {
            line.contains(&format!(" {}{parameter}", operation.method)) && line.contains(&call)
        });
        assert!(declared, "{}: {}", op.id, operation.method);
        for paginator in operation.paginators.iter().flatten() {
            let input = operation.input.as_ref().expect("paginated input");
            let declared = client.lines().any(|line| {
                line.contains(&format!(
                    " {paginator}(models.{} input, {{",
                    input.type_name
                )) && line.contains(&format!("_runtime.pages(\"{}\"", op.name()))
            });
            assert!(declared, "{}: {paginator}", op.id);
        }
        for (structure, style) in [
            (&operation.input, "keywords"),
            (&operation.output, "properties"),
        ] {
            let Some(structure) = structure else { continue };
            assert_eq!(structure.style, style);
            let block = class_block(models, &structure.type_name);
            for member in &structure.members {
                let field = &member.name;
                let declared = block.lines().find_map(|line| {
                    let line = line.trim();
                    if let Some(ty) = line
                        .strip_prefix("final ")
                        .and_then(|rest| rest.strip_suffix(&format!(" {field};")))
                    {
                        return Some(ty.trim_end_matches('?').to_owned());
                    }
                    line.strip_suffix(&format!(" get {field} => _{field}Presence != null ? _{field}Presence.value : _{field};"))
                        .map(|ty| ty.trim_end_matches('?').to_owned())
                });
                let declared =
                    declared.unwrap_or_else(|| panic!("{}.{field} declared", structure.type_name));
                assert_eq!(
                    caller(&declared),
                    member.type_name,
                    "{}.{field}",
                    structure.type_name
                );
                let required = block.contains(&format!("required this.{field}"));
                let optional = block.contains(&format!("? {field}, wire.Field<"));
                assert!(required || optional, "{}({field}:)", structure.type_name);
                assert_eq!(
                    optional, member.optional,
                    "{}.{field} optional",
                    structure.type_name
                );
            }
        }
    }
}

#[test]
fn surface_names_what_the_package_declares() {
    let service = load("weather");
    let options = options(Some("https://weather.example"));
    let files = emit(&service, &options).unwrap();
    let client = file(&files, "lib/src/client.dart");
    let surface = surface(&service, &options);

    assert_eq!(surface.language, "dart");
    assert_eq!(surface.package.ecosystem, "pub");
    assert_eq!(surface.package.name, "weather_client");
    assert_eq!(
        surface.package.import,
        "package:weather_client/weather_client.dart"
    );
    // GetCityPhoto's blob is a `Uint8List`, which the package does not export.
    assert_eq!(
        surface.package.imports,
        [
            "import 'package:weather_client/weather_client.dart';",
            "import 'dart:typed_data';"
        ]
    );
    assert_eq!(surface.package.qualifier, "");
    assert!(files.iter().any(|f| f.path == "lib/weather_client.dart"));
    let library = file(&files, "lib/weather_client.dart");
    for export in [
        "export 'src/client.dart';",
        "export 'src/models.dart'",
        "export 'src/runtime/config.dart';",
        "export 'src/runtime/timestamp.dart';",
        "show JsonValue, JsonNumber, BigDecimal;",
    ] {
        assert!(library.contains(export), "{export}");
    }
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(surface.client.construct, "WeatherClient()");
    assert!(!surface.client.statements);
    assert!(!surface.client.fallible);
    // The configuration is optional, so the generated endpoint applies.
    assert!(client.contains("  WeatherClient([ClientConfig? config])"));
    assert!(client.contains("endpoint: \"https://weather.example\""));
    assert_eq!(surface.endpoint.as_deref(), Some("https://weather.example"));

    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "listCities");
    assert_eq!(list.paginators, Some(vec!["listCitiesPages".to_owned()]));
    assert_eq!(
        surface.operations["example.weather#GetCity"].paginators,
        None
    );
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, "ListCitiesInput");
    let max = input
        .members
        .iter()
        .find(|m| m.member == "maxResults")
        .unwrap();
    assert_eq!(
        (max.name.as_str(), max.type_name.as_str(), max.optional),
        ("maxResults", "int", true)
    );
    let create = surface.operations["example.weather#CreateCity"]
        .input
        .as_ref()
        .unwrap();
    let revision = create
        .members
        .iter()
        .find(|m| m.member == "expectedRevision")
        .unwrap();
    assert_eq!(revision.type_name, "BigInt");
    assert!(
        !create
            .members
            .iter()
            .find(|m| m.member == "name")
            .unwrap()
            .optional
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
        "WeatherClient(ClientConfig(endpoint: endpoint))"
    );
    assert_eq!(surface.endpoint, None);
    // `ClientConfig` takes the endpoint as a named parameter.
    let config = crucible_codegen_dart::emit(&service, &options).unwrap();
    let config = file(&config, "lib/src/runtime/config.dart");
    assert!(config.contains("  ClientConfig(\n      {this.endpoint,"));
    assert_declared(&service, &options, &surface);
}

/// Another package name moves the library and its import.
#[test]
fn the_package_name_decides_the_import() {
    let service = load("weather");
    let options = Options {
        package_name: "acme_forecasts".into(),
        ..options(None)
    };
    let surface = surface(&service, &options);
    assert_eq!(
        surface.package.import,
        "package:acme_forecasts/acme_forecasts.dart"
    );
    assert_eq!(
        surface.package.imports[0],
        "import 'package:acme_forecasts/acme_forecasts.dart';"
    );
    let files = emit(&service, &options).unwrap();
    assert!(files.iter().any(|f| f.path == "lib/acme_forecasts.dart"));
    assert_declared(&service, &options, &surface);
}

/// Reserved words are renamed as the models declare them (`inValue`).
#[test]
fn keyword_members_are_named_as_declared() {
    let service = load("client-docs-surface-keywords");
    let options = options(None);
    let surface = surface(&service, &options);
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
    for keyword in ["in", "default", "class"] {
        assert_ne!(name(keyword), keyword, "{keyword} is reserved in Dart");
    }
    assert_eq!(name("in"), "inValue");
    assert_declared(&service, &options, &surface);
}

#[test]
fn derived_name_collisions_are_refused_not_suffixed() {
    let service = load("client-docs-surface-collisions");
    let diagnostics = match emit(&service, &options(None)) {
        Ok(_) => panic!("ListThingsPages collides with listThings' Pages stream"),
        Err(diagnostics) => diagnostics.to_string(),
    };
    assert!(
        diagnostics.contains("Dart operation name collision"),
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

/// Each snippet is a whole program over the names the package declares:
/// its import, its client, its method or paginator, its input class, and
/// the error class it catches.
#[test]
fn snippets_call_what_the_package_declares() {
    for (model, endpoint) in [
        ("weather", None),
        ("samples", Some("https://samples.example")),
    ] {
        let service = load(model);
        let options = options(endpoint);
        let surface = surface(&service, &options);
        let files = emit(&service, &options).unwrap();
        let client = file(&files, "lib/src/client.dart");
        let models = file(&files, "lib/src/models.dart");
        for op in &service.operations {
            let entry = &surface.operations[&op.id.to_string()];
            let snippet = &entry.snippet;
            assert!(
                snippet.contains("import 'package:weather_client/weather_client.dart';"),
                "{snippet}"
            );
            assert!(snippet.contains("Future<void> main() async {"), "{snippet}");
            assert!(snippet.contains(&format!("await client.{}(", entry.method)));
            assert!(snippet.contains(&format!("final client = {}", surface.client.name)));
            assert!(snippet.contains("client.close();"));
            if let Some(input) = &entry.input {
                assert!(snippet.contains(&format!("final input = {}(", input.type_name)));
            }
            let call = crucible_codegen_common::snippet::call(&service, op);
            if let Some(error) = call.error {
                let name = service.name_of(error.id);
                assert!(snippet.contains(&format!("}} on {name} catch (error) {{")));
                assert!(models.contains(&format!("final class {name} extends")));
            }
            assert_eq!(
                entry.paginated_snippet.is_some(),
                entry.paginators.is_some(),
                "{}",
                op.id
            );
            if let (Some(pages), Some(paginators)) = (&entry.paginated_snippet, &entry.paginators) {
                assert!(pages.contains(&format!(
                    "await for (final page in client.{}(",
                    paginators[0]
                )));
                assert!(client.contains(&format!(" {}(", paginators[0])));
            }
        }
        // Without a generated endpoint the client reads it, and the bearer
        // token, from the environment.
        let get = &surface.operations.values().next().unwrap().snippet;
        assert_eq!(get.contains("import 'dart:io' as io;"), endpoint.is_none());
    }
    let service = load("weather");
    let surface = surface(&service, &options(None));
    let get = &surface.operations["example.weather#GetCity"].snippet;
    for line in [
        "  final client = WeatherClient(ClientConfig(\n    endpoint: io.Platform.environment['WEATHER_ENDPOINT'],\n    token: () => io.Platform.environment['WEATHER_TOKEN'],\n  ));",
        "  // Values from the model's example \"Read a city\".",
        "    at: Timestamp.parse(\"2026-01-02T03:04:05Z\"),",
        "    print(output.requestId);",
    ] {
        assert!(get.contains(line), "{line}\n{get}");
    }
}

/// The README, `AGENTS.md` and dartdoc's configuration are generated
/// beside the pubspec, and the README shows a complete call.
#[test]
fn the_package_carries_its_readme_agents_and_dartdoc_options() {
    let service = load("weather");
    let options = options(None);
    let files = emit(&service, &options).unwrap();
    let readme = file(&files, "README.md");
    assert!(readme.contains("dart pub add weather_client:0.0.0-dev"));
    assert!(readme.contains("await client.getCity(input)"));
    assert!(readme.contains("await for (final page in client.listCitiesPages(input))"));
    let agents = file(&files, "AGENTS.md");
    assert!(agents.contains("`WeatherClient`"));
    assert!(file(&files, "dartdoc_options.yaml").contains("dartdoc:"));
}
