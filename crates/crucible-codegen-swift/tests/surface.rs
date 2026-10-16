//! The surface names exactly what the generated package declares: each
//! method on its own operation, each member in its own model struct, and
//! types that resolve after the surface's imports.

use std::path::{Path, PathBuf};

use crucible_codegen_common::surface::Surface;
use crucible_codegen_swift::{Options, emit, surface};
use crucible_ir::ir::Service;

fn model_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn load(name: &str) -> Service {
    crucible_lang::load_service(&model_dir(name)).unwrap_or_else(|e| panic!("{name}:\n{e}"))
}

/// The keyword fixture with a model named `class` beside its keyword-named
/// members.
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

fn options(module: &str, endpoint: Option<&str>) -> Options {
    Options {
        package_name: format!("{module}SDK"),
        module_name: module.into(),
        package_version: "0.0.0-dev".into(),
        endpoint: endpoint.map(str::to_owned),
    }
}

/// A name as the generated code declares it: always in backticks.
fn declared(name: &str) -> String {
    format!("`{}`", name.trim_matches('`'))
}

/// The text from the line holding `start` to the end of its block.
fn block<'s>(source: &'s str, start: &str, end: &str) -> &'s str {
    let at = source.find(start).unwrap_or_else(|| panic!("no {start:?}"));
    let stop = source[at..]
        .find(end)
        .map_or(source.len(), |stop| at + stop);
    &source[at..stop]
}

/// `ty` as a caller reads it, for comparing a declared type with the
/// surface's: backticks and `Swift.` qualifiers removed.
fn normalized(ty: &str) -> String {
    ty.replace('`', "").replace("Swift.", "")
}

/// Every name the surface gives is declared by the emitted package, where
/// it belongs.
fn assert_declared(service: &Service, options: &Options, surface: &Surface) {
    let module = &options.module_name;
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
        ["import Foundation".to_owned(), format!("import {module}")]
    );
    assert_eq!(surface.package.qualifier, "");
    let client = file(&format!("Sources/{module}/{}.swift", surface.client.name));
    let models = file(&format!("Sources/{module}/Models.swift"));
    assert!(client.contains(&format!(
        "public final class {}: Sendable {{",
        surface.client.name
    )));
    // `try` because the emitted initializer throws.
    assert!(client.contains("    public init(_ config: ClientConfig = ClientConfig()) throws {"));
    assert!(surface.client.construct.starts_with("try "));
    assert!(surface.client.fallible && !surface.client.statements);
    let config = files
        .iter()
        .find(|file| file.path == format!("Sources/{module}/ClientConfig.swift"))
        .expect("the runtime's configuration");
    assert!(
        config
            .contents
            .contains("public struct ClientConfig: Sendable {")
    );
    assert!(
        config
            .contents
            .contains("    public init(endpoint: String? = nil,")
    );
    assert_eq!(surface.operations.len(), service.operations.len());
    for operation in &service.operations {
        let id = operation.id.to_string();
        let entry = &surface.operations[&id];
        let input = entry
            .input
            .as_ref()
            .map_or("Unit".to_owned(), |input| declared(&input.type_name));
        // The method takes its own input type and calls its own operation.
        let method = block(
            client,
            &format!(
                "    public func {}(_ input: {input}",
                declared(&entry.method)
            ),
            "\n    }\n",
        );
        assert!(
            method.contains(&format!(
                "try await runtime.call(\"{}\", input: input",
                operation.name()
            )),
            "{id}: {}",
            entry.method
        );
        assert_eq!(entry.paginators.is_some(), operation.paginated.is_some());
        for paginator in entry.paginators.iter().flatten() {
            let pages = block(
                client,
                &format!("    public func {}(_ input: {input})", declared(paginator)),
                "\n    }\n",
            );
            assert!(
                pages.contains(&format!(
                    "try runtime.pages(\"{}\", input: input",
                    operation.name()
                )),
                "{id}: {paginator}"
            );
        }
        for (structure, style) in [(&entry.input, "keywords"), (&entry.output, "properties")] {
            let Some(structure) = structure else { continue };
            assert_eq!(structure.style, style);
            let body = block(
                models,
                &format!("    public struct {}: ", declared(&structure.type_name)),
                "\n    }\n",
            );
            let init = body
                .lines()
                .find(|line| line.trim_start().starts_with("public init("))
                .unwrap();
            for member in &structure.members {
                let prefix = format!("        public var {}: ", declared(&member.name));
                let line = body
                    .lines()
                    .find(|line| line.starts_with(&prefix))
                    .unwrap_or_else(|| panic!("{id}: {}.{}", structure.type_name, member.name));
                let ty = line[prefix.len()..].trim_end_matches(" {");
                assert_eq!(
                    normalized(ty.strip_suffix('?').unwrap_or(ty)),
                    normalized(&member.type_name),
                    "{id}: {}.{}",
                    structure.type_name,
                    member.name
                );
                assert_eq!(member.optional, ty.ends_with('?'), "{id}: {}", member.name);
                // A caller may leave out exactly the parameters with `= nil`.
                let parameter = format!("{}: ", declared(&member.name));
                let at = init.find(&parameter).unwrap_or_else(|| panic!("{init}"));
                let parameter = init[at..].split([',', ')']).next().unwrap();
                assert_eq!(
                    member.optional,
                    parameter.ends_with(" = nil"),
                    "{id}: {parameter}"
                );
                // Each member is an initializer label.
                assert!(
                    init.contains(&format!("{}: ", declared(&member.name))),
                    "{init}"
                );
            }
        }
    }
}

#[test]
fn weather_surface_names_what_the_package_declares() {
    let service = load("weather");
    let options = options("Weather", None);
    let surface = surface(&service, &options);
    assert_eq!(surface.language, "swift");
    assert_eq!(surface.package.ecosystem, "swiftpm");
    assert_eq!(surface.package.name, "WeatherSDK");
    assert_eq!(surface.package.import, "Weather");
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(
        surface.client.construct,
        "try WeatherClient(ClientConfig(endpoint: endpoint))"
    );
    assert_declared(&service, &options, &surface);
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "listCities");
    assert_eq!(
        list.paginators.as_deref(),
        Some(&["listCitiesPages".to_owned()][..])
    );
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, "ListCitiesInput");
    assert_eq!(input.members[1].name, "maxResults");
    assert_eq!(input.members[1].type_name, "Int32");
    assert!(input.members[1].optional);
}

/// Another module moves the imports; the names stay, and a generated-in
/// endpoint leaves nothing for the caller to supply.
#[test]
fn a_different_module_moves_the_imports() {
    let service = load("weather");
    let options = options("Forecasts", Some("https://weather.example"));
    let surface = surface(&service, &options);
    assert_eq!(surface.package.import, "Forecasts");
    assert_eq!(surface.package.name, "ForecastsSDK");
    assert_eq!(surface.client.construct, "try WeatherClient()");
    assert!(!surface.client.construct.contains("endpoint"));
    assert_eq!(surface.endpoint.as_deref(), Some("https://weather.example"));
    assert_declared(&service, &options, &surface);
}

/// Keyword-named members keep the backticks Swift needs; `self` is a
/// renamed member, and a model named `class` takes its `Model` suffix.
#[test]
fn keyword_names_keep_the_backticks_swift_needs() {
    let options = options("Keywords", None);
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
            "`in`",
            "object",
            "type",
            "`default`",
            "selfValue",
            "`class`",
            "fun",
            "def",
            "`func`",
            "end",
            "when",
            "`struct`"
        ]
    );
    let service = keywords_with_class();
    let surface = crucible_codegen_swift::surface(&service, &options);
    assert_declared(&service, &options, &surface);
    let output = surface.operations["example.keywords#Describe"]
        .output
        .as_ref()
        .unwrap();
    assert_eq!(output.members[0].name, "detail");
    assert_eq!(output.members[0].type_name, "classModel");
}

/// Swift refuses colliding method names rather than suffixing them, so the
/// suffixed names other targets publish never reach a Swift package.
#[test]
fn colliding_operation_names_are_refused() {
    let service = load("client-docs-surface-collisions");
    let Err(diagnostics) = emit(&service, &options("Weather", None)) else {
        panic!("colliding method names are refused");
    };
    assert!(
        diagnostics
            .to_string()
            .contains("Swift operation name collision"),
        "{diagnostics}"
    );
}

/// Swift's initializer takes every member that has no presence, a
/// defaulted one too: only `Presence` members may be left out.
#[test]
fn only_presence_members_may_be_left_out() {
    let service = load("client-docs-surface-defaults");
    let options = options("Defaults", None);
    let surface = surface(&service, &options);
    assert_declared(&service, &options, &surface);
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
            ("count".to_owned(), false),
            ("mode".to_owned(), false),
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

/// Each snippet is a whole program over the names the package declares:
/// the module import, the client, the operation's method on its own input
/// type, each output property, and the paginator over the same input.
#[test]
fn snippets_call_what_the_package_declares() {
    for (name, module) in [("weather", "Weather"), ("samples", "Samples")] {
        let service = load(name);
        let options = options(module, None);
        let surface = surface(&service, &options);
        assert_declared(&service, &options, &surface);
        let models = emit(&service, &options)
            .unwrap()
            .into_iter()
            .find(|file| file.path == format!("Sources/{module}/Models.swift"))
            .unwrap()
            .contents;
        for operation in &service.operations {
            let entry = &surface.operations[&operation.id.to_string()];
            let snippet = &entry.snippet;
            let id = &operation.id;
            assert!(
                snippet.starts_with(&format!("import Foundation\nimport {module}\n")),
                "{id}"
            );
            assert!(
                snippet.contains(&format!("let client = try {}(", surface.client.name)),
                "{id}"
            );
            assert!(
                snippet.contains(&format!("try await client.{}(", entry.method)),
                "{id}"
            );
            assert!(snippet.ends_with("client.close()\n"), "{id}");
            if let Some(input) = &entry.input {
                assert!(
                    snippet.contains("let input = ")
                        && snippet.contains(&format!("{}(", input.type_name)),
                    "{id}: {snippet}"
                );
                assert!(
                    models.contains(&format!("public struct {}: ", declared(&input.type_name)))
                );
            }
            for member in entry.output.iter().flat_map(|output| &output.members) {
                assert!(
                    snippet.contains(&format!("output.{}", member.name)),
                    "{id}: {}",
                    member.name
                );
            }
            if let Some(caught) = snippet
                .lines()
                .find_map(|line| line.strip_prefix("} catch let error as "))
            {
                let error = caught.trim_end_matches(" {");
                assert!(
                    models.contains(&format!("public struct {}: ", declared(error))),
                    "{id}: {error}"
                );
            }
            match (&entry.paginators, &entry.paginated_snippet) {
                (Some(paginators), Some(pages)) => {
                    assert!(pages.contains(&format!(
                        "for try await page in try client.{}(",
                        paginators[0]
                    )));
                    // The paginator sends the token: the input leaves it out.
                    let token = &operation.paginated.as_ref().unwrap().input_token;
                    assert!(!pages.contains(&format!("    {token}: ")), "{id}");
                }
                (None, None) => {}
                _ => panic!("{id}: a paginated snippet exactly for a paginated operation"),
            }
        }
        // Both snippets read their credentials from the environment.
        let get = &surface.operations.values().next().unwrap().snippet;
        assert!(get.contains("ProcessInfo.processInfo.environment["));
        // The same model gives the same text.
        assert_eq!(surface, crucible_codegen_swift::surface(&service, &options));
    }
    let weather = surface(&load("weather"), &options("Weather", None));
    let get = &weather.operations["example.weather#GetCity"].snippet;
    assert!(get.contains("environment[\"WEATHER_TOKEN\"]"), "{get}");
    assert!(get.contains("environment[\"WEATHER_ENDPOINT\"]"), "{get}");
    assert!(get.contains("// Values from the model's example \"Read a city\"."));
    assert!(get.contains("} catch let error as NotFoundError {"));
    // A generated-in endpoint is not read from the environment.
    let fixed = surface(
        &load("weather"),
        &options("Weather", Some("https://weather.example")),
    );
    assert!(
        !fixed.operations["example.weather#GetCity"]
            .snippet
            .contains("WEATHER_ENDPOINT")
    );
}
