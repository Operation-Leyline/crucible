//! The surface names what the generated gem declares.

use crucible_codegen_common::surface::Surface;
use crucible_codegen_ruby::{File, Options, emit, surface};
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
        module_name: "Weather".into(),
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

/// `CreateCityInput` is `create_city_input.rb`.
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (index, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && index > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Every name the surface gives is declared where it says: each method and
/// paginator on the client, documented as taking its operation's input and
/// calling that operation; each member as an attribute reader of its own
/// model class, documented with the stated type (module-relative inside
/// the module), and as a keyword argument that defaults to unset exactly
/// when the member is optional.
fn assert_declared(service: &Service, options: &Options, surface: &Surface) {
    let files = emit(service, options).unwrap();
    let package = &options.package_name;
    let qualifier = &surface.package.qualifier;
    let bare = |name: &str| {
        name.strip_prefix(qualifier.as_str())
            .unwrap_or_else(|| panic!("{name} is qualified"))
            .to_owned()
    };
    let client = file(&files, &format!("lib/{package}/client.rb"));
    assert!(client.contains(&format!("module {}\n", options.module_name)));
    assert!(client.contains(&format!("  class {}\n", bare(&surface.client.name))));
    assert_eq!(surface.operations.len(), service.operations.len());
    for op in &service.operations {
        let operation = &surface.operations[&op.id.to_string()];
        let (parameters, value) = match &operation.input {
            Some(_) => ("input, cancellation: nil", "input"),
            None => ("cancellation: nil", "Runtime::Unit.new"),
        };
        let method = format!(
            "    def {}({parameters})\n      @runtime.call('{}', {value}, cancellation: cancellation)\n",
            operation.method,
            op.name()
        );
        let at = client
            .find(&method)
            .unwrap_or_else(|| panic!("{}: {method}", op.id));
        // Its YARD block, between the previous method and this one.
        let docs = &client[client[..at].rfind("    end\n").unwrap_or(0)..at];
        if let Some(input) = &operation.input {
            let documented = format!("@param input [{}]", bare(&input.type_name));
            assert!(docs.contains(&documented), "{}: {documented}", op.id);
        }
        for paginator in operation.paginators.iter().flatten() {
            let method = format!(
                "    def {paginator}(input, cancellation: nil)\n      @runtime.pages('{}', input, cancellation: cancellation)\n",
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
            let name = bare(&structure.type_name);
            let model = file(&files, &format!("lib/{package}/models/{}.rb", snake(&name)));
            assert!(model.contains(&format!("    class {name}")), "{name}");
            let initialize = model
                .lines()
                .find(|line| line.trim_start().starts_with("def initialize("))
                .unwrap_or_else(|| panic!("{name}.new"));
            for member in &structure.members {
                let field = &member.name;
                assert!(
                    model.contains(&format!("      attr_reader :{field}\n")),
                    "{name}#{field}"
                );
                let required = initialize.contains(&format!("({field}:,"))
                    || initialize.contains(&format!(" {field}:,"))
                    || initialize.contains(&format!(" {field}:)"))
                    || initialize.contains(&format!("({field}:)"));
                let optional = initialize.contains(&format!(
                    "{field}: ::{}::Runtime::UNSET",
                    options.module_name
                ));
                assert!(required || optional, "{name}.new({field}:) in {initialize}");
                assert_eq!(
                    optional, member.optional,
                    "{name}.new({field}:) in {initialize}"
                );
                // The YARD type the generated docs state, module-relative.
                let documented = format!(
                    "@return [{}",
                    member.type_name.replace(qualifier.as_str(), "")
                );
                assert!(model.contains(&documented), "{name}#{field}: {documented}");
            }
        }
    }
}

#[test]
fn surface_names_what_the_gem_declares() {
    let service = load("weather");
    let options = options(Some("https://weather.example"));
    let files = emit(&service, &options).unwrap();
    let surface = surface(&service, &options);

    assert_eq!(surface.language, "ruby");
    assert_eq!(surface.package.ecosystem, "rubygems");
    assert_eq!(surface.package.name, "weather_client");
    assert_eq!(surface.package.import, "weather_client");
    assert_eq!(surface.package.imports, ["require 'weather_client'"]);
    assert_eq!(surface.package.qualifier, "Weather::");
    assert!(files.iter().any(|f| f.path == "lib/weather_client.rb"));
    assert!(files.iter().any(|f| f.path == "weather_client.gemspec"));
    assert_eq!(surface.client.name, "Weather::WeatherClient");
    assert_eq!(surface.client.construct, "Weather::WeatherClient.new");
    assert!(!surface.client.statements);
    assert!(!surface.client.fallible);
    assert_eq!(surface.endpoint.as_deref(), Some("https://weather.example"));

    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "list_cities");
    assert_eq!(list.paginators, Some(vec!["list_cities_pages".to_owned()]));
    assert_eq!(
        surface.operations["example.weather#GetCity"].paginators,
        None
    );
    let output = list.output.as_ref().unwrap();
    assert_eq!(output.type_name, "Weather::ListCitiesOutput");
    let cities = output
        .members
        .iter()
        .find(|m| m.member == "cities")
        .unwrap();
    assert_eq!(cities.type_name, "Array<Weather::CitySummary>");
    assert!(!cities.optional);
    // Runtime value classes keep their namespace, so they resolve.
    let summary = &surface.operations["example.weather#CreateCity"];
    let summary = summary.output.as_ref().unwrap();
    assert_eq!(summary.members[0].type_name, "Weather::CitySummary");
    let forecast = surface.operations["example.weather#GetCity"]
        .output
        .as_ref()
        .unwrap()
        .members
        .iter()
        .find(|m| m.member == "forecast")
        .unwrap()
        .type_name
        .clone();
    // The map alias reads as its Hash, holding the runtime's JSON values.
    assert_eq!(forecast, "Hash{String => Weather::Runtime::JsonValue}");
    assert_declared(&service, &options, &surface);
}

#[test]
fn construction_reads_an_endpoint_when_none_is_generated() {
    let service = load("weather");
    let options = options(None);
    let surface = surface(&service, &options);
    assert_eq!(
        surface.client.construct,
        "Weather::WeatherClient.new(Weather::Runtime::ClientConfig.new(endpoint: endpoint))"
    );
    // The client takes a `Runtime::ClientConfig`, whose `endpoint` is a
    // keyword argument.
    let files = emit(&service, &options).unwrap();
    let client = file(&files, "lib/weather_client/client.rb");
    assert!(client.contains("    def initialize(config = Runtime::ClientConfig.new)\n"));
    let config = file(&files, "lib/weather_client/runtime/client_config.rb");
    assert!(config.contains("def initialize(endpoint: nil,"));
    assert_declared(&service, &options, &surface);
}

/// Another module qualifies every name with it; the gem name is the
/// require path.
#[test]
fn the_module_qualifies_every_name() {
    let service = load("weather");
    let options = Options {
        package_name: "acme_forecasts".into(),
        module_name: "Acme".into(),
        ..options(None)
    };
    let surface = surface(&service, &options);
    assert_eq!(surface.package.imports, ["require 'acme_forecasts'"]);
    assert_eq!(surface.package.qualifier, "Acme::");
    assert_eq!(surface.client.name, "Acme::WeatherClient");
    let created = surface.operations["example.weather#GetCity"]
        .output
        .as_ref()
        .unwrap();
    assert_eq!(created.type_name, "Acme::GetCityOutput");
    assert_declared(&service, &options, &surface);
}

/// Weather with a timestamp member in `CreateCity`'s input.
fn weather_with_timestamp() -> Service {
    let mut package = crucible_lang::load_package(&model_dir("weather")).unwrap();
    for file in &mut package.files {
        file.text = file.text.replace("\r\n", "\n").replace(
            "    strict input {\n",
            "    strict input {\n        at?: timestamp\n",
        );
    }
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    compiled.service.unwrap_or_else(|| panic!("{rendered}"))
}

/// Timestamps are runtime classes inside the module.
#[test]
fn runtime_value_types_keep_their_namespace() {
    let service = weather_with_timestamp();
    let options = options(None);
    let surface = surface(&service, &options);
    let create = surface.operations["example.weather#CreateCity"]
        .input
        .as_ref()
        .unwrap();
    let at = create.members.iter().find(|m| m.member == "at").unwrap();
    assert_eq!(at.type_name, "Weather::Runtime::Timestamp");
    assert_declared(&service, &options, &surface);
}

/// Reserved words are renamed as the models declare them (`in_value`).
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
    for keyword in ["in", "class", "def", "end", "when", "self"] {
        assert_ne!(name(keyword), keyword, "{keyword} is reserved in Ruby");
    }
    assert_eq!(name("in"), "in_value");
    assert_declared(&service, &options, &surface);
}

#[test]
fn derived_name_collisions_are_refused_not_suffixed() {
    let service = load("client-docs-surface-collisions");
    let diagnostics = match emit(&service, &options(None)) {
        Ok(_) => panic!("ListThingsPages collides with list_things' pages enumerator"),
        Err(diagnostics) => diagnostics.to_string(),
    };
    assert!(
        diagnostics.contains("Ruby operation name collision"),
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
