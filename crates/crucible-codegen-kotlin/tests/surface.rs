//! The surface names exactly what the generated package declares: each
//! method on its own operation, each member in its own model class, and
//! types that resolve after the surface's imports.

use std::path::{Path, PathBuf};

use crucible_codegen_common::surface::Surface;
use crucible_codegen_kotlin::{Options, emit, surface};
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

fn options(package: &str, endpoint: Option<&str>) -> Options {
    Options {
        group_id: "com.example".into(),
        artifact_id: "weather-client".into(),
        package_name: package.into(),
        package_version: "0.0.0-SNAPSHOT".into(),
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
/// surface's: backticks, the model package and default-imported `kotlin.`
/// qualifiers removed.
fn normalized(ty: &str, package: &str) -> String {
    let ty = ty
        .replace('`', "")
        .replace(&format!("{package}.model."), "");
    match ty.strip_prefix("kotlin.") {
        Some(rest) if !rest.contains('.') => rest.to_owned(),
        _ => ty,
    }
}

/// Every name the surface gives is declared by the emitted package, where
/// it belongs.
fn assert_declared(service: &Service, options: &Options, surface: &Surface) {
    let package = &options.package_name;
    let files = emit(service, options).unwrap();
    let source = format!("src/commonMain/kotlin/{}", package.replace('.', "/"));
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
        [
            format!("import {package}.{}", surface.client.name),
            format!("import {package}.ClientConfig"),
            format!("import {package}.model.*"),
        ]
    );
    assert_eq!(surface.package.qualifier, "");
    let client = file(&format!("{source}/{}.kt", surface.client.name));
    assert!(client.contains(&format!("package {package}\n")));
    assert!(client.contains(&format!(
        "class {}(config: ClientConfig)",
        surface.client.name
    )));
    let config = files
        .iter()
        .find(|file| file.path.ends_with("/ClientConfig.kt"))
        .expect("the runtime's configuration");
    assert!(config.contents.contains(&format!("package {package}\n")));
    assert!(
        config
            .contents
            .contains("data class ClientConfig(\n    val endpoint: String? = null,")
    );
    assert!(!surface.client.statements && !surface.client.fallible);
    assert_eq!(surface.operations.len(), service.operations.len());
    for operation in &service.operations {
        let id = operation.id.to_string();
        let entry = &surface.operations[&id];
        assert_eq!(entry.async_method, None);
        let (parameter, argument) = match &entry.input {
            Some(input) => (
                format!("input: {package}.model.{}", declared(&input.type_name)),
                "input",
            ),
            None => (String::new(), "null"),
        };
        // The snippet is a `main` calling this method, the paginated one
        // collecting its page flow; every model it builds is declared.
        let snippet_argument = if entry.input.is_some() { "input" } else { "" };
        assert!(entry.snippet.contains("suspend fun main() {\n"), "{id}");
        assert!(
            entry
                .snippet
                .contains(&format!("client.{}({snippet_argument})", entry.method)),
            "{id}: {}",
            entry.snippet
        );
        assert_eq!(
            entry.paginated_snippet.is_some(),
            operation.paginated.is_some(),
            "{id}"
        );
        if let (Some(snippet), Some(pages)) = (&entry.paginated_snippet, &entry.paginators) {
            assert!(snippet.contains(&format!("client.{}(", pages[0])), "{id}");
        }
        for snippet in std::iter::once(&entry.snippet).chain(&entry.paginated_snippet) {
            for (at, _) in snippet.match_indices(" {\n") {
                let line = &snippet[snippet[..at].rfind('\n').map_or(0, |n| n + 1)..at];
                let Some(name) = line
                    .rsplit([' ', '('])
                    .next()
                    .filter(|name| name.starts_with(|c: char| c.is_ascii_uppercase()))
                else {
                    continue;
                };
                assert!(
                    files.iter().any(|file| file.path
                        == format!(
                            "{source}/model/{}.kt",
                            name.trim_end_matches(')').trim_matches('`')
                        )),
                    "{id}: {name}"
                );
            }
        }
        // The method takes its own input type and calls its own operation.
        let method = format!("    suspend fun {}({parameter}): ", declared(&entry.method));
        let line = client
            .lines()
            .find(|line| line.starts_with(&method))
            .unwrap_or_else(|| panic!("{id}: no {method}"));
        assert!(
            line.contains(&format!(
                "runtime.call(\"{}\", {argument})",
                operation.name()
            )),
            "{id}: {line}"
        );
        assert_eq!(entry.paginators.is_some(), operation.paginated.is_some());
        for paginator in entry.paginators.iter().flatten() {
            let pages = block(
                client,
                &format!("    fun {}({parameter}): ", declared(paginator)),
                "\n    }\n",
            );
            assert!(
                pages.contains(&format!("runtime.pages(\"{}\", input)", operation.name())),
                "{id}: {paginator}"
            );
        }
        for (structure, input) in [(&entry.input, true), (&entry.output, false)] {
            let Some(structure) = structure else { continue };
            assert_eq!(structure.style, if input { "dsl" } else { "properties" });
            let bare = structure.type_name.trim_matches('`');
            let model = file(&format!("{source}/model/{bare}.kt"));
            let class = block(
                model,
                &format!("class {} internal constructor(", declared(bare)),
                "\n}\n",
            );
            // Inputs are set on the DSL's builder; outputs are read from
            // the model's properties.
            let scope = if input {
                block(class, "    class Builder {", "\n    }\n")
            } else {
                class
            };
            for member in &structure.members {
                let prefix = if input {
                    format!("        var {}: ", declared(&member.name))
                } else {
                    format!("    val {}: ", declared(&member.name))
                };
                let line = scope
                    .lines()
                    .find(|line| line.starts_with(&prefix))
                    .unwrap_or_else(|| panic!("{id}: {bare}.{}", member.name));
                let ty = line[prefix.len()..].split([',', ' ', '?']).next().unwrap();
                assert_eq!(
                    normalized(ty, package),
                    normalized(&member.type_name, package),
                    "{id}: {bare}.{}",
                    member.name
                );
                let optional = if input {
                    // The builder documents what `build()` requires.
                    let at = scope.find(line).unwrap();
                    let doc = &scope[scope[..at].rfind("/**").unwrap_or(0)..at];
                    !doc.contains("Required: `build()` fails")
                } else {
                    line.contains("? get()")
                };
                assert_eq!(member.optional, optional, "{id}: {}", member.name);
            }
        }
    }
}

#[test]
fn weather_surface_names_what_the_package_declares() {
    let service = load("weather");
    let options = options("example.weather", None);
    let surface = surface(&service, &options);
    assert_eq!(surface.language, "kotlin");
    assert_eq!(surface.package.ecosystem, "maven");
    assert_eq!(surface.package.name, "com.example:weather-client");
    assert_eq!(surface.package.import, "example.weather");
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(
        surface.client.construct,
        "WeatherClient(ClientConfig(endpoint = endpoint))"
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
    assert_eq!(input.members[2].name, "tags");
    assert_eq!(input.members[2].type_name, "Tags");
    assert_eq!(input.members[1].type_name, "Int");
    assert!(input.members[1].optional);
    let city = surface.operations["example.weather#GetCity"]
        .output
        .as_ref()
        .unwrap();
    assert!(!city.members[0].optional);
    assert_eq!(city.members[2].name, "requestIdValue");
}

/// Another package moves the imports; platform types that are not
/// default-imported stay qualified.
#[test]
fn a_different_package_moves_the_imports() {
    let service = load("weather");
    let options = options("com.acme.forecasts", Some("https://weather.example"));
    let surface = surface(&service, &options);
    assert_eq!(surface.package.import, "com.acme.forecasts");
    assert_eq!(surface.client.construct, "WeatherClient(ClientConfig())");
    assert!(!surface.client.construct.contains("endpoint"));
    assert_eq!(surface.endpoint.as_deref(), Some("https://weather.example"));
    assert_declared(&service, &options, &surface);
    let summary = surface.operations["example.weather#CreateCity"]
        .output
        .as_ref()
        .unwrap();
    assert_eq!(summary.members[0].type_name, "CitySummary");
}

/// Keyword-named members and a model class named `class` keep the
/// backticks Kotlin needs; `class` itself is a renamed member.
#[test]
fn keyword_names_keep_the_backticks_kotlin_needs() {
    let options = options("example.keywords", None);
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
            "`object`",
            "type",
            "default",
            "self",
            "classValue",
            "`fun`",
            "def",
            "func",
            "end",
            "`when`",
            "struct"
        ]
    );
    let service = keywords_with_class();
    let surface = crucible_codegen_kotlin::surface(&service, &options);
    assert_declared(&service, &options, &surface);
    let output = surface.operations["example.keywords#Describe"]
        .output
        .as_ref()
        .unwrap();
    assert_eq!(output.members[0].name, "detail");
    assert_eq!(output.members[0].type_name, "`class`");
}

/// Kotlin refuses colliding method names rather than suffixing them, so the
/// suffixed names other targets publish never reach a Kotlin package.
#[test]
fn colliding_operation_names_are_refused() {
    let service = load("client-docs-surface-collisions");
    let Err(diagnostics) = emit(&service, &options("example.weather", None)) else {
        panic!("colliding method names are refused");
    };
    assert!(
        diagnostics
            .to_string()
            .contains("Kotlin client method name collision"),
        "{diagnostics}"
    );
}

/// `build()` fills a defaulted input member left unset, so the DSL may
/// leave it out; an output's defaulted member is always present.
#[test]
fn defaulted_input_members_are_omittable() {
    let service = load("client-docs-surface-defaults");
    let options = options("example.defaults", None);
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
