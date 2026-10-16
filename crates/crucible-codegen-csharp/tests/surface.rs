//! The surface names exactly what the generated package declares: each
//! method on its own operation, each member in its own model class, and
//! types that resolve after the surface's `using` lines.

use std::path::{Path, PathBuf};

use crucible_codegen_common::surface::Surface;
use crucible_codegen_csharp::{Options, emit, surface};
use crucible_ir::ir::Service;

fn model_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn load(name: &str) -> Service {
    crucible_lang::load_service(&model_dir(name)).unwrap_or_else(|e| panic!("{name}:\n{e}"))
}

/// The keyword fixture with a model class named `class` beside its
/// keyword-named members.
fn keywords_with_class() -> Service {
    let mut package =
        crucible_lang::load_package(&model_dir("client-docs-surface-keywords")).unwrap();
    for file in &mut package.files {
        file.text = file
            .text
            .replace("\r\n", "\n")
            .replace("    output {\n", "    output {\n        detail?: class\n");
    }
    package.files.push(crucible_lang::SourceFile {
        path: "class.cru".into(),
        text: "structure class {\n    value?: string\n}\n".into(),
    });
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    compiled
        .service
        .unwrap_or_else(|| panic!("the edited fixture compiles:\n{rendered}"))
}

fn options(namespace: &str, endpoint: Option<&str>) -> Options {
    Options {
        namespace: namespace.into(),
        package_id: format!("{namespace}.Client"),
        package_version: "0.0.0-dev".into(),
        endpoint: endpoint.map(str::to_owned),
    }
}

/// A name as the generated code declares it: always `@`-prefixed.
fn declared(name: &str) -> String {
    format!("@{}", name.trim_start_matches('@'))
}

/// The text from the line holding `start` to the end of its class.
fn block<'s>(source: &'s str, start: &str) -> &'s str {
    let at = source.find(start).unwrap_or_else(|| panic!("no {start:?}"));
    let end = source[at..]
        .find("\n}\n")
        .map_or(source.len(), |end| at + end);
    &source[at..end]
}

/// `ty` as a caller reads it: namespaces and `@` removed, so a declared
/// type compares with the surface's.
fn normalized(ty: &str, ns: &str) -> String {
    ty.replace(&format!("global::{ns}.Models."), "")
        .replace(&format!("global::{ns}."), "")
        .replace("global::", "")
        .replace('@', "")
}

/// Every name the surface gives is declared by the emitted package, where
/// it belongs.
fn assert_declared(service: &Service, options: &Options, surface: &Surface) {
    let ns = &options.namespace;
    let files = emit(service, options).unwrap();
    let file = |path: &str| {
        files
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| panic!("no {path}"))
            .contents
            .as_str()
    };
    assert_eq!(
        surface.package.imports,
        [format!("using {ns};"), format!("using {ns}.Models;")]
    );
    assert_eq!(surface.package.qualifier, "");
    // The construction expression calls the emitted constructor with the
    // runtime's configuration, in the root namespace the imports open.
    let client = file(&format!("{}.cs", surface.client.name));
    assert!(client.contains(&format!("namespace {ns};")));
    assert!(client.contains(&format!(
        "public sealed class {} : global::System.IDisposable {{",
        surface.client.name
    )));
    assert!(client.contains(&format!(
        "    public {}(ClientConfig config) {{",
        surface.client.name
    )));
    let config = file("ClientConfig.cs");
    assert!(config.contains(&format!("namespace {ns};")));
    assert!(config.contains("public sealed class ClientConfig"));
    assert!(config.contains("public string? Endpoint { get; init; }"));
    assert!(!surface.client.statements);
    assert!(!surface.client.fallible);
    assert_eq!(surface.operations.len(), service.operations.len());
    for operation in &service.operations {
        let id = operation.id.to_string();
        let entry = &surface.operations[&id];
        let input = entry
            .input
            .as_ref()
            .map(|input| format!("global::{ns}.Models.{} input, ", declared(&input.type_name)));
        let parameters = input.clone().unwrap_or_default();
        let wire = format!("(\"{}\", ", operation.name());
        let line = |name: &str| {
            client
                .lines()
                .find(|line| {
                    line.trim_start().starts_with("public ")
                        && line.contains(&format!(" {}({parameters}", declared(name)))
                })
                .unwrap_or_else(|| panic!("{id}: no {name}({parameters}"))
        };
        let async_method = entry.async_method.as_deref().unwrap();
        // The asynchronous form sends the operation; the blocking form
        // runs that same asynchronous method.
        assert!(line(async_method).contains(&wire), "{id}: {async_method}");
        let argument = if input.is_some() { "input, " } else { "" };
        assert!(
            line(&entry.method).contains(&format!(
                "{}({argument}cancellationToken)",
                declared(async_method)
            )),
            "{id}: {}",
            entry.method
        );
        assert_eq!(entry.paginators.is_some(), operation.paginated.is_some());
        for paginator in entry.paginators.iter().flatten() {
            let line = line(paginator);
            assert!(
                line.contains("PagesAsync<") && line.contains(&wire),
                "{id}: {paginator}"
            );
        }
        for (structure, style) in [(&entry.input, "fields"), (&entry.output, "properties")] {
            let Some(structure) = structure else { continue };
            assert_eq!(structure.style, style);
            let bare = structure.type_name.trim_start_matches('@');
            let model = file(&format!("Models/{bare}.cs"));
            let class = block(model, &format!("public sealed class {} {{", declared(bare)));
            for member in &structure.members {
                let property = class
                    .lines()
                    .find(|line| {
                        line.trim_start().starts_with("public ")
                            && line.contains(&format!(" {} {{ ", declared(&member.name)))
                    })
                    .unwrap_or_else(|| panic!("{id}: {bare}.{}", member.name));
                let ty = property.trim_start()["public ".len()..]
                    .split(&format!(" {} {{", declared(&member.name)))
                    .next()
                    .unwrap();
                let ty = normalized(ty, ns);
                assert_eq!(
                    ty.strip_suffix('?').unwrap_or(&ty),
                    member.type_name.replace('@', ""),
                    "{id}: {bare}.{}",
                    member.name
                );
                assert_eq!(
                    member.optional,
                    property.contains("Presence"),
                    "{id}: {}",
                    member.name
                );
            }
        }
    }
}

#[test]
fn weather_surface_names_what_the_package_declares() {
    let service = load("weather");
    let options = options("Example.Weather", None);
    let surface = surface(&service, &options);
    assert_eq!(surface.language, "csharp");
    assert_eq!(surface.package.ecosystem, "nuget");
    assert_eq!(surface.package.name, "Example.Weather.Client");
    assert_eq!(surface.package.import, "Example.Weather");
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(
        surface.client.construct,
        "new WeatherClient(new ClientConfig { Endpoint = endpoint })"
    );
    assert_eq!(surface.endpoint, None);
    assert_declared(&service, &options, &surface);
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "ListCities");
    assert_eq!(list.async_method.as_deref(), Some("ListCitiesAsync"));
    assert_eq!(
        list.paginators.as_deref(),
        Some(&["ListCitiesPagesAsync".to_owned()][..])
    );
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, "ListCitiesInput");
    assert_eq!(input.members[0].name, "NextToken");
    assert_eq!(input.members[0].type_name, "string");
    assert!(input.members[0].optional);
    let create = surface.operations["example.weather#CreateCity"]
        .output
        .as_ref()
        .unwrap();
    assert_eq!(create.members[0].type_name, "CitySummary");
    let city = &surface.operations["example.weather#GetCity"];
    let forecast = &city.output.as_ref().unwrap().members[1];
    assert_eq!(
        forecast.type_name,
        "System.Collections.Generic.Dictionary<Sky, System.Text.Json.Nodes.JsonNode>"
    );
}

/// Another namespace moves the imports and the qualifiers stripped with
/// them; the names stay.
#[test]
fn a_different_namespace_moves_the_imports() {
    let service = load("weather");
    let options = options("Acme.Forecasts", Some("https://weather.example"));
    let surface = surface(&service, &options);
    assert_eq!(surface.package.import, "Acme.Forecasts");
    assert_eq!(surface.package.name, "Acme.Forecasts.Client");
    assert_eq!(
        surface.client.construct,
        "new WeatherClient(new ClientConfig())"
    );
    assert!(!surface.client.construct.contains("endpoint"));
    assert_eq!(surface.endpoint.as_deref(), Some("https://weather.example"));
    assert_declared(&service, &options, &surface);
}

/// Keyword-named members are PascalCase properties, which need no escape;
/// a model class named `class` keeps the `@` its declaration needs.
#[test]
fn keyword_names_keep_the_escapes_csharp_needs() {
    let options = options("Example.Keywords", None);
    let service = load("client-docs-surface-keywords");
    let surface = surface(&service, &options);
    assert_declared(&service, &options, &surface);
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
    let service = keywords_with_class();
    let surface = crucible_codegen_csharp::surface(&service, &options);
    assert_declared(&service, &options, &surface);
    let output = surface.operations["example.keywords#Describe"]
        .output
        .as_ref()
        .unwrap();
    assert_eq!(output.members[0].name, "Detail");
    assert_eq!(output.members[0].type_name, "@class");
}

/// C# refuses colliding method names rather than suffixing them, so the
/// suffixed names other targets publish never reach a C# package.
#[test]
fn colliding_operation_names_are_refused() {
    let service = load("client-docs-surface-collisions");
    let Err(diagnostics) = emit(&service, &options("Example.Weather", None)) else {
        panic!("colliding method names are refused");
    };
    assert!(
        diagnostics
            .to_string()
            .contains("C# client method name collision"),
        "{diagnostics}"
    );
}
