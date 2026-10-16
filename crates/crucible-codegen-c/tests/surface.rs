//! The surface (`crucible.sdks/1`) names what the generated package
//! declares: every operation function (taking its operation's input and
//! calling that operation), page iterator, model and accessor, with the
//! types the declarations spell, and the statements that construct a
//! client from the runtime's declarations.

use crucible_codegen_c::{Options, emit, surface};
use crucible_codegen_common::surface::Surface;
use crucible_ir::ir::Service;

fn options() -> Options {
    Options {
        package_name: "weather_client".into(),
        prefix: "weather".into(),
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

fn file<'a>(files: &'a [crucible_codegen_c::File], path: &str) -> &'a str {
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

/// Asserts that every name the surface gives is declared in the package
/// the generator emits for `service` with `options`, and returns it.
fn assert_declared(service: &Service, options: &Options) -> Surface {
    let files = emit(service, options).unwrap();
    let package = &options.package_name;
    let p = &options.prefix;
    let client = file(&files, &format!("include/{package}/client.h"));
    let source = file(&files, "src/client.c");
    let models = file(&files, &format!("include/{package}/models.h"));
    let runtime = file(
        &files,
        &format!("include/{package}/runtime/client_runtime.h"),
    );
    let types = file(&files, &format!("include/{package}/runtime/types.h"));
    let descriptors = file(&files, "src/models.c");
    let surface = surface(service, options);
    // A member is optional exactly when its descriptor says so: the flag
    // the runtime reads to accept it absent ("Missing required member").
    let descriptor_optional = |owner: &str, accessor: &str, kind: &str| {
        let field = accessor
            .strip_prefix(&format!("{owner}_{kind}_"))
            .unwrap_or_else(|| panic!("{accessor} is {owner}'s {kind}ter"));
        let value = format!("offsetof({owner}, {field}.value), ");
        let line = descriptors
            .lines()
            .find(|line| line.contains(&value))
            .unwrap_or_else(|| panic!("{owner}.{field} has a descriptor"));
        let flag = &line[line.find(&value).unwrap() + value.len()..];
        match flag.split(',').next() {
            Some("true") => true,
            Some("false") => false,
            other => panic!("{owner}.{field}: {other:?}"),
        }
    };
    assert_eq!(surface.package.import, format!("{package}/client.h"));
    assert_eq!(
        surface.package.imports,
        [format!("#include \"{package}/client.h\"")]
    );
    assert_eq!(surface.package.qualifier, "");
    assert!(surface.client.statements && surface.client.fallible);

    // `construct` declares `config`, `client` and `error` with the
    // runtime's types and calls the generated constructor.
    let name = &surface.client.name;
    assert_eq!(name, &format!("{p}_client"));
    assert!(client.contains(&format!(
        "{p}_status {name}_new(const {name}_config *config, {name} **output, {p}_error *error);"
    )));
    assert!(runtime.contains(&format!("{name}_config {name}_defaults(void);")));
    let end = runtime.find(&format!("}} {name}_config;")).unwrap();
    let config = &runtime[runtime[..end].rfind("typedef struct").unwrap()..end];
    assert!(
        config.contains(&format!("{p}_string endpoint;")),
        "{config}"
    );
    assert!(types.contains(&format!("}} {p}_error;")));
    let set_endpoint = match &options.endpoint {
        Some(_) => String::new(),
        None => {
            assert!(types.contains(&format!("{p}_string {p}_string_view(const char *text);")));
            format!(" config.endpoint = {p}_string_view(endpoint);")
        }
    };
    assert_eq!(
        surface.client.construct,
        format!(
            "{name}_config config = {name}_defaults();{set_endpoint} {name} *client = NULL; {p}_error error = {{0}}; {name}_new(&config, &client, &error);"
        )
    );

    assert_eq!(surface.operations.len(), service.operations.len());
    for operation in &service.operations {
        let entry = &surface.operations[&operation.id.to_string()];
        let function = &entry.method;
        let input = entry.input.as_ref().map_or(String::new(), |input| {
            format!("const {} *input, ", input.type_name)
        });
        let operation_name = format!("\"{}\", {}}}", operation.name(), operation.name().len());
        // Declared with this operation's input, and defined to call this
        // operation.
        let signature = format!(
            "{p}_status {function}({p}_client *client, {input}{p}_cancelled_fn cancelled, void *cancel_user, {function}_result *result, {p}_error *error)"
        );
        assert!(client.contains(&format!("{signature};")), "{signature}");
        assert!(
            block(source, &format!("{signature} {{"), "\n}\n").contains(&operation_name),
            "{function} calls {}",
            operation.name()
        );
        if let Some(output) = &entry.output {
            assert!(client.contains(&format!(
                "typedef struct {{ {p}_result base; const {} *value; }} {function}_result;",
                output.type_name
            )));
        }
        assert_eq!(entry.paginators.is_some(), operation.paginated.is_some());
        if let Some(paginators) = &entry.paginators {
            let [pages_new, pages_next] = &paginators[..] else {
                panic!("{function}: a constructor and a step");
            };
            let signature = format!(
                "{p}_status {pages_new}({p}_client *client, {input}{p}_cancelled_fn cancelled, void *cancel_user, {p}_pages **pages, {p}_error *error)"
            );
            assert!(client.contains(&format!("{signature};")), "{signature}");
            assert!(block(source, &format!("{signature} {{"), "\n}\n").contains(&operation_name));
            assert!(client.contains(&format!(
                "{p}_status {pages_next}({p}_pages *pages, bool *has_page, {function}_result *result, {p}_error *error);"
            )));
        }
        if let Some(input) = &entry.input {
            assert_eq!(input.style, "setters");
            assert!(models.contains(&format!("\nstruct {} {{", input.type_name)));
            for member in &input.members {
                let setter = format!(
                    "static inline void {}({} *model, {} value)",
                    member.name, input.type_name, member.type_name
                );
                assert!(models.contains(&setter), "{setter}");
                assert_eq!(
                    member.optional,
                    descriptor_optional(&input.type_name, &member.name, "set"),
                    "{} optional",
                    member.name
                );
            }
        }
        if let Some(output) = &entry.output {
            assert_eq!(output.style, "functions");
            assert!(models.contains(&format!("\nstruct {} {{", output.type_name)));
            for member in &output.members {
                let getter = format!(
                    "static inline {} {}(const {} *model)",
                    member.type_name, member.name, output.type_name
                );
                assert!(models.contains(&getter), "{getter}");
                assert_eq!(
                    member.optional,
                    descriptor_optional(&output.type_name, &member.name, "get"),
                    "{} optional",
                    member.name
                );
            }
        }
    }
    surface
}

#[test]
fn weather_surface_names_the_generated_declarations() {
    let surface = assert_declared(&load("weather"), &options());
    assert_eq!(surface.language, "c");
    assert_eq!(surface.package.ecosystem, "cmake");
    assert_eq!(surface.package.name, "weather_client");
    assert_eq!(surface.client.name, "weather_client");
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "weather_operations_list_cities");
    assert_eq!(
        list.paginators,
        Some(vec![
            "weather_operations_list_cities_pages_new".to_owned(),
            "weather_operations_list_cities_pages_next".to_owned(),
        ])
    );
    let output = list.output.as_ref().unwrap();
    assert_eq!(output.type_name, "weather_models_list_cities_output");
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
        revision.name,
        "weather_models_create_city_input_set_expected_revision"
    );
    assert!(revision.optional);
    assert!(!create.members[0].optional, "name is required");
}

/// Another prefix and package, with a generated-in endpoint: every symbol
/// follows the prefix, and constructing sets no endpoint.
#[test]
fn another_prefix_names_every_symbol() {
    let options = Options {
        package_name: "forecast".into(),
        prefix: "fc".into(),
        package_version: "0.0.0".into(),
        endpoint: Some("https://api.weather.example".into()),
    };
    let surface = assert_declared(&load("weather"), &options);
    assert!(!surface.client.construct.contains("endpoint"));
    assert_eq!(surface.client.name, "fc_client");
    assert_eq!(
        surface.endpoint.as_deref(),
        Some("https://api.weather.example")
    );
    assert_eq!(
        surface.operations["example.weather#GetCity"].method,
        "fc_operations_get_city"
    );
}

/// Members named for C keywords take the `_value` suffix the accessors
/// declare (`..._set_default_value`, `..._set_struct_value`); the others
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
    assert_eq!(
        name("default"),
        "weather_models_describe_input_set_default_value"
    );
    assert_eq!(
        name("struct"),
        "weather_models_describe_input_set_struct_value"
    );
    assert_eq!(name("in"), "weather_models_describe_input_set_in");
}

/// C's `_pages_new`/`_pages_next` suffixes leave the derived names other
/// generators collide on (`ListThingsPages`, `PaginateListThings`) free, so
/// the fixture generates, and the surface names what it declares.
#[test]
fn derived_names_beside_colliding_operations_are_declared() {
    let surface = assert_declared(&load("client-docs-surface-collisions"), &options());
    let list = &surface.operations["example.surface#ListThings"];
    assert_eq!(
        list.paginators,
        Some(vec![
            "weather_operations_list_things_pages_new".to_owned(),
            "weather_operations_list_things_pages_next".to_owned(),
        ])
    );
    assert_eq!(
        surface.operations["example.surface#ListThingsPages"].method,
        "weather_operations_list_things_pages"
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
