//! The surface (`crucible.sdks/1`) names what the generated package
//! declares: every client method (taking its operation's input and calling
//! that operation), paginator, model, constructor argument and field, with
//! the types the declarations spell.

use crucible_codegen_common::surface::Surface;
use crucible_codegen_cpp::{Options, emit, surface};
use crucible_ir::ir::Service;

fn options() -> Options {
    Options {
        package_name: "weather_client".into(),
        namespace: "weather".into(),
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

fn file<'a>(files: &'a [crucible_codegen_cpp::File], path: &str) -> &'a str {
    &files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("no {path}"))
        .contents
}

/// The text from `start` to the first `end` after it.
fn block<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let at = text.find(start).unwrap_or_else(|| panic!("no {start:?}"));
    let length = text[at..]
        .find(end)
        .unwrap_or_else(|| panic!("{start:?} ends"));
    &text[at..at + length]
}

/// A surface type as the generated code spells it, from the global
/// namespace.
fn generated(options: &Options, ty: &str) -> String {
    let ns = &options.namespace;
    ty.replace(&format!("{ns}::"), &format!("::{ns}::"))
}

/// Asserts that every name the surface gives is declared in the package
/// the generator emits for `service` with `options`, and returns it.
fn assert_declared(service: &Service, options: &Options) -> Surface {
    let files = emit(service, options).unwrap();
    let package = &options.package_name;
    let ns = &options.namespace;
    let header = file(&files, &format!("include/{package}/client.hpp"));
    let source = file(&files, "src/client.cpp");
    let models = file(&files, &format!("include/{package}/models.hpp"));
    let runtime = file(
        &files,
        &format!("include/{package}/runtime/client_runtime.hpp"),
    );
    let surface = surface(service, options);
    assert_eq!(surface.package.import, format!("{package}/client.hpp"));
    assert_eq!(
        surface.package.imports,
        [format!("#include \"{package}/client.hpp\"")]
    );
    assert_eq!(surface.package.qualifier, format!("{ns}::"));
    assert!(!surface.client.statements && !surface.client.fallible);

    // The client class, in its namespace, and the constructor `construct`
    // calls: one optional ClientConfig whose first field is the endpoint.
    let class = surface
        .client
        .name
        .strip_prefix(&surface.package.qualifier)
        .expect("the client is in the package's namespace");
    assert!(header.contains(&format!("\nnamespace {ns} {{\n")));
    assert!(header.contains(&format!("class {class} {{")));
    assert!(header.contains(&format!(
        "explicit {class}(runtime::ClientConfig config = {{}});"
    )));
    let config = block(runtime, "struct ClientConfig {", "\n};");
    let first = config
        .lines()
        .find(|line| !line.trim_start().starts_with("/**") && line.contains(';'))
        .unwrap();
    assert_eq!(first.trim(), "std::optional<std::string> endpoint;");
    let constructor = format!("{0}::{0}(runtime::ClientConfig config)", class);
    let generated_endpoint = block(source, &constructor, "{}\n");
    match &options.endpoint {
        Some(endpoint) => {
            assert_eq!(
                surface.client.construct,
                format!("{}()", surface.client.name)
            );
            assert!(generated_endpoint.contains(&format!("\"{endpoint}\"")));
        }
        None => {
            assert_eq!(
                surface.client.construct,
                format!(
                    "{}({ns}::runtime::ClientConfig{{.endpoint = endpoint}})",
                    surface.client.name
                )
            );
            assert!(generated_endpoint.contains("decode_error, \"\")"));
        }
    }

    assert_eq!(surface.operations.len(), service.operations.len());
    for operation in &service.operations {
        let entry = &surface.operations[&operation.id.to_string()];
        let output = entry
            .output
            .as_ref()
            .map_or("runtime::Unit".to_owned(), |output| {
                generated(options, &output.type_name)
            });
        let input = entry.input.as_ref().map_or(String::new(), |input| {
            format!("const {}& input, ", generated(options, &input.type_name))
        });
        let call = format!("runtime_->call(\"{}\", ", operation.name());
        // Declared with this operation's input and output, and defined to
        // call this operation.
        let declaration = format!(
            "    [[nodiscard]] {output} {}({input}std::stop_token stop = {{}}) const;\n",
            entry.method
        );
        assert!(header.contains(&declaration), "{declaration}");
        let definition = format!(
            "{output} {class}::{}({input}std::stop_token stop) const {{\n",
            entry.method
        );
        assert!(
            block(source, &definition, "\n}\n").contains(&call),
            "{definition}"
        );
        assert_eq!(entry.paginators.is_some(), operation.paginated.is_some());
        for paginator in entry.paginators.iter().flatten() {
            let declaration = format!(
                "    [[nodiscard]] runtime::Pages<{output}> {paginator}({input}std::stop_token stop = {{}}) const;\n"
            );
            assert!(header.contains(&declaration), "{declaration}");
            let definition = format!(
                "runtime::Pages<{output}> {class}::{paginator}({input}std::stop_token stop) const {{\n"
            );
            assert!(
                block(source, &definition, "\n}\n")
                    .contains(&format!("(runtime_, \"{}\", ", operation.name())),
                "{definition}"
            );
        }
        for (structure, style) in entry
            .input
            .iter()
            .map(|input| (input, "positional"))
            .chain(entry.output.iter().map(|output| (output, "fields")))
        {
            assert_eq!(structure.style, style);
            let name = structure
                .type_name
                .strip_prefix(&format!("{ns}::models::"))
                .expect("a model in the models namespace");
            let body = block(models, &format!("\nstruct {name} {{\n"), "\n};");
            // Each field, wrapped in `Field` exactly when it is optional.
            for member in &structure.members {
                let ty = generated(options, &member.type_name);
                let field = if member.optional {
                    format!("    ::{ns}::runtime::Field<{ty}> {};\n", member.name)
                } else {
                    format!("    {ty} {};\n", member.name)
                };
                assert!(body.contains(&field), "{name}: {field}");
            }
            // The constructor takes the required members, then the
            // optional ones, each in model order.
            let constructor = block(body, &format!("explicit {name}("), ")");
            let order: Vec<&str> = structure
                .members
                .iter()
                .filter(|member| !member.optional)
                .chain(structure.members.iter().filter(|member| member.optional))
                .map(|member| member.name.as_str())
                .collect();
            let mut at = 0;
            for parameter in order {
                let found = [", ", " = ", ")"].iter().find_map(|end| {
                    constructor[at..]
                        .find(&format!(" {parameter}{end}"))
                        .or_else(|| {
                            constructor[at..]
                                .ends_with(&format!(" {parameter}"))
                                .then(|| constructor.len() - at - parameter.len() - 1)
                        })
                });
                let found = found
                    .unwrap_or_else(|| panic!("{name}: {parameter} after {at} in {constructor}"));
                at += found + parameter.len();
            }
        }
    }
    surface
}

#[test]
fn weather_surface_names_the_generated_declarations() {
    let service = load("weather");
    let surface = assert_declared(&service, &options());
    assert_eq!(surface.language, "cpp");
    assert_eq!(surface.package.ecosystem, "cmake");
    assert_eq!(surface.package.name, "weather_client");
    assert_eq!(surface.client.name, "weather::WeatherClient");
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "list_cities");
    assert_eq!(list.paginators, Some(vec!["list_cities_pages".to_owned()]));
    assert_eq!(
        list.input.as_ref().unwrap().type_name,
        "weather::models::ListCitiesInput"
    );
    let create = surface.operations["example.weather#CreateCity"]
        .input
        .as_ref()
        .unwrap();
    let types: Vec<(&str, &str, bool)> = create
        .members
        .iter()
        .map(|member| {
            (
                member.name.as_str(),
                member.type_name.as_str(),
                member.optional,
            )
        })
        .collect();
    assert_eq!(
        types,
        [
            ("name", "std::string", false),
            ("location", "weather::models::Location", true),
            ("client_token", "std::string", true),
            ("expected_revision", "std::int64_t", true),
        ]
    );
}

/// Another namespace and package: the names follow the options.
#[test]
fn a_nested_namespace_qualifies_every_name() {
    let options = Options {
        package_name: "forecast".into(),
        namespace: "example::weather".into(),
        package_version: "0.0.0".into(),
        endpoint: Some("https://api.weather.example".into()),
    };
    let surface = assert_declared(&load("weather"), &options);
    assert_eq!(
        surface.package.imports,
        ["#include \"forecast/client.hpp\""]
    );
    assert_eq!(surface.client.name, "example::weather::WeatherClient");
    assert_eq!(
        surface.client.construct,
        "example::weather::WeatherClient()"
    );
    assert_eq!(
        surface.endpoint.as_deref(),
        Some("https://api.weather.example")
    );
    assert_eq!(
        surface.operations["example.weather#GetCity"]
            .output
            .as_ref()
            .unwrap()
            .type_name,
        "example::weather::models::GetCityOutput"
    );
}

/// Members named for C++ keywords take the `_value` suffix the models
/// declare (`class_value`, `default_value`, `struct_value`); the others
/// keep their names.
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
    assert_eq!(name("class"), "class_value");
    assert_eq!(name("default"), "default_value");
    assert_eq!(name("struct"), "struct_value");
    assert_eq!(name("in"), "in");
}

/// C++ diagnoses a derived method that collides with an operation's
/// instead of suffixing it, so a surface never names a renamed method.
#[test]
fn colliding_derived_methods_are_refused() {
    let service = load("client-docs-surface-collisions");
    let diagnostics = emit(&service, &options()).err().expect("refused");
    assert!(
        diagnostics
            .to_string()
            .contains("C++ operation name collision"),
        "{diagnostics}"
    );
}
