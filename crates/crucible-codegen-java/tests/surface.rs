//! The surface names what the generated package declares.

use std::path::Path;

use crucible_codegen_common::surface::Surface;
use crucible_codegen_java::{File, Options, emit, surface};

fn load(name: &str) -> crucible_ir::ir::Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&dir).unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

fn options(package: &str, endpoint: Option<&str>) -> Options {
    Options {
        group_id: "example".into(),
        artifact_id: "weather-client".into(),
        package_name: package.into(),
        package_version: "0.0.0-SNAPSHOT".into(),
        endpoint: endpoint.map(str::to_owned),
    }
}

fn source(files: &[File], suffix: &str) -> String {
    files
        .iter()
        .find(|file| file.path.ends_with(suffix))
        .unwrap_or_else(|| panic!("no {suffix}"))
        .contents
        .clone()
}

/// The method declared by `header`, through its closing brace.
fn method<'a>(code: &'a str, header: &str) -> &'a str {
    let start = code.find(header).unwrap_or_else(|| panic!("no {header}"));
    let rest = &code[start..];
    &rest[..rest.find("\n    }\n").map_or(rest.len(), |at| at + 6)]
}

/// Every name the surface gives is declared where it should be: each
/// method taking its operation's input and calling its own operation, each
/// setter inside its model's builder and each accessor on the model, with
/// the member's type.
fn assert_declared(service: &crucible_ir::ir::Service, options: &Options, surface: &Surface) {
    let files = emit(service, options).unwrap();
    let package = &options.package_name;
    let qualifier = &surface.package.qualifier;
    assert_eq!(qualifier, &format!("{package}.model."));
    let name = &surface.client.name;
    assert_eq!(
        surface.package.imports,
        [
            format!("import {package}.ClientConfig;"),
            format!("import {package}.{name};")
        ]
    );
    let client = source(&files, &format!("/{name}.java"));
    assert!(
        client.starts_with(&format!("package {package};"))
            || client.contains(&format!("\npackage {package};"))
    );
    assert!(client.contains(&format!("public final class {name} {{")));
    assert!(client.contains(&format!("public {name}(ClientConfig config)")));
    // The construct fills the runtime's `ClientConfig` through its public
    // `endpoint` field and declares `client`.
    let config = source(&files, "/ClientConfig.java");
    assert!(config.contains(&format!("package {package};")));
    assert!(config.contains("  public ClientConfig() {}"));
    assert!(surface.client.statements);
    if surface
        .client
        .construct
        .contains("config.endpoint = endpoint;")
    {
        assert!(config.contains("  public String endpoint;"));
    } else {
        assert!(!surface.client.construct.contains("endpoint"));
    }
    assert!(
        surface
            .client
            .construct
            .ends_with(&format!("{name} client = new {name}(config);"))
            || surface
                .client
                .construct
                .ends_with(&format!("{name} client = new {name}(new ClientConfig());"))
    );
    assert_eq!(surface.operations.len(), service.operations.len());
    for (id, operation) in &surface.operations {
        let op = service
            .operations
            .iter()
            .find(|op| op.id.to_string() == *id)
            .unwrap();
        let (parameter, argument) = match &operation.input {
            Some(input) => (format!("{} input", input.type_name), "input"),
            None => (String::new(), "null"),
        };
        let wire = format!("\"{}\"", op.name());
        let asynchronous = operation.async_method.as_deref().unwrap();
        let body = method(&client, &format!(" {asynchronous}({parameter}) {{\n"));
        assert!(
            body.contains(&format!("runtime.call({wire}, {argument}, ")),
            "{id}: {asynchronous} calls {wire}"
        );
        let blocking = method(&client, &format!(" {}({parameter}) {{\n", operation.method));
        let call = if operation.input.is_some() {
            "input"
        } else {
            ""
        };
        assert!(
            blocking.contains(&format!("ClientRuntime.await({asynchronous}({call}))")),
            "{id}: {} awaits {asynchronous}",
            operation.method
        );
        let calls = ["runtime.pages", "runtime.pagesAsync"];
        for (pages, call) in operation.paginators.iter().flatten().zip(calls) {
            let body = method(&client, &format!(" {pages}({parameter}) {{\n"));
            assert!(
                body.contains(&format!("{call}({wire}, {argument}, ")),
                "{id}: {pages} pages through {wire}"
            );
        }
        for structure in [&operation.input, &operation.output].into_iter().flatten() {
            let local = structure
                .type_name
                .strip_prefix(qualifier.as_str())
                .unwrap();
            let model = source(&files, &format!("/model/{local}.java")).replace("java.lang.", "");
            assert!(model.contains(&format!("public final class {local}")));
            let builder = model
                .find("public static final class Builder {")
                .unwrap_or(model.len());
            let (fields, setters) = model.split_at(builder);
            for member in &structure.members {
                let (declaration, scope) = if structure.style == "builder" {
                    (
                        format!("public Builder {}({} item)", member.name, member.type_name),
                        setters,
                    )
                } else {
                    (
                        format!("public {} {}() {{", member.type_name, member.name),
                        fields,
                    )
                };
                assert!(
                    scope.contains(&declaration),
                    "{local}.{}: {declaration}",
                    member.name
                );
                // The field says whether the member may be unset: an
                // `OptionalValue` starts unset, a defaulted field starts
                // at its default, a bare one is required. Outputs here
                // are not errors, so their accessor is the field's name.
                let field = fields
                    .lines()
                    .map(str::trim)
                    .find(|line| {
                        let declaration = line.split(" = ").next().unwrap();
                        line.starts_with("public ")
                            && declaration.trim_end_matches(';').rsplit(' ').next()
                                == Some(member.name.as_str())
                    })
                    .unwrap_or_else(|| panic!("{local}.{}: field", member.name));
                let unset = field.contains(".OptionalValue<");
                let optional = if structure.style == "builder" {
                    unset || field.contains(" = ")
                } else {
                    unset
                };
                assert_eq!(
                    member.optional, optional,
                    "{local}.{}: {field}",
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
        "client-docs-jvm",
        "client-docs-surface-keywords",
        "client-docs-surface-defaults",
        "extended-types",
    ] {
        let service = load(model);
        for endpoint in [Some("https://weather.example"), None] {
            let options = options("example.weather", endpoint);
            let surface = surface(&service, &options);
            assert_eq!(surface.language, "java");
            assert_eq!(surface.package.ecosystem, "maven");
            assert_eq!(surface.package.name, "example:weather-client");
            assert_eq!(surface.package.import, "example.weather");
            assert_declared(&service, &options, &surface);
        }
    }
}

/// A member named like the package's first segment takes `member_field`'s
/// `_`, in the builder and the accessor alike.
#[test]
fn a_member_named_like_the_package_root_is_renamed() {
    let service = load("weather");
    let options = options("name.weather", None);
    let surface = surface(&service, &options);
    let create = surface.operations["example.weather#CreateCity"]
        .input
        .as_ref()
        .unwrap();
    assert_eq!(create.members[0].member, "name");
    assert_eq!(create.members[0].name, "name_");
    assert_eq!(create.type_name, "name.weather.model.CreateCityInput");
    assert_declared(&service, &options, &surface);
}

#[test]
fn weather_surface_spells_java() {
    let service = load("weather");
    let surface = surface(&service, &options("example.weather", None));
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(
        surface.client.construct,
        "ClientConfig config = new ClientConfig();\nconfig.endpoint = endpoint;\nWeatherClient client = new WeatherClient(config);"
    );
    assert!(surface.client.statements && !surface.client.fallible);
    let configured = crucible_codegen_java::surface(
        &service,
        &options("example.weather", Some("https://x.example")),
    );
    assert_eq!(
        configured.client.construct,
        "WeatherClient client = new WeatherClient(new ClientConfig());"
    );
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "listCities");
    assert_eq!(list.async_method.as_deref(), Some("listCitiesAsync"));
    assert_eq!(
        list.paginators,
        Some(vec![
            "listCitiesPages".to_owned(),
            "listCitiesPagesAsync".to_owned()
        ])
    );
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, "example.weather.model.ListCitiesInput");
    assert_eq!(input.style, "builder");
    assert!(!input.fallible);
    assert_eq!(input.members[0].name, "nextToken");
    assert_eq!(input.members[0].type_name, "String");
    assert!(input.members[0].optional);
    assert_eq!(list.output.as_ref().unwrap().style, "methods");
    assert!(
        surface.operations["example.weather#Ping"]
            .paginators
            .is_none()
    );
}

/// Reserved words take the emitter's `_` suffix.
#[test]
fn keywords_take_the_emitted_suffix() {
    let surface = surface(
        &load("client-docs-surface-keywords"),
        &options("example.keywords", None),
    );
    let input = surface.operations["example.keywords#Describe"]
        .input
        .as_ref()
        .unwrap();
    let names: Vec<_> = input.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "in", "object", "type", "default_", "self", "class_", "fun", "def", "func", "end",
            "when", "struct"
        ]
    );
}

/// Java refuses method collisions instead of suffixing them, so there is
/// never a surface for a package that does not generate.
#[test]
fn colliding_method_names_are_refused() {
    let service = load("client-docs-surface-collisions");
    assert!(emit(&service, &options("example.weather", None)).is_err());
}

/// A builder may leave a defaulted input member unset (its field starts at
/// the default); an output's defaulted member is always present.
#[test]
fn defaulted_input_members_are_omittable() {
    let surface = surface(
        &load("client-docs-surface-defaults"),
        &options("example.defaults", None),
    );
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
