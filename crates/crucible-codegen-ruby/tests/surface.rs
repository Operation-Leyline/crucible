//! The surface names what the generated gem declares.

use crucible_codegen_ruby::{File, Options, emit, surface};
use std::path::{Path, PathBuf};

fn model_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
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

#[test]
fn surface_names_what_the_gem_declares() {
    let service = crucible_lang::load_service(&model_dir("weather")).unwrap();
    let options = options(Some("https://weather.example"));
    let files = emit(&service, &options).unwrap();
    let surface = surface(&service, &options);

    assert_eq!(surface.language, "ruby");
    assert_eq!(surface.package.ecosystem, "rubygems");
    assert_eq!(surface.package.name, "weather_client");
    assert_eq!(surface.package.import, "weather_client");
    assert!(files.iter().any(|f| f.path == "lib/weather_client.rb"));
    assert!(files.iter().any(|f| f.path == "weather_client.gemspec"));
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(surface.client.construct, "Weather::WeatherClient.new");
    assert_eq!(surface.endpoint.as_deref(), Some("https://weather.example"));
    let client = file(&files, "lib/weather_client/client.rb");
    assert!(client.contains("module Weather\n"));
    assert!(client.contains("  class WeatherClient\n"));

    assert_eq!(surface.operations.len(), service.operations.len());
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "list_cities");
    assert_eq!(list.paginators, Some(vec!["list_cities_pages".to_owned()]));
    assert_eq!(
        surface.operations["example.weather#GetCity"].paginators,
        None
    );
    for operation in surface.operations.values() {
        assert!(
            client.contains(&format!("    def {}(", operation.method)),
            "{}",
            operation.method
        );
        for paginator in operation.paginators.iter().flatten() {
            assert!(
                client.contains(&format!("    def {paginator}(input")),
                "{paginator}"
            );
        }
        for (structure, style) in [
            (&operation.input, "keywords"),
            (&operation.output, "properties"),
        ] {
            let Some(structure) = structure else { continue };
            assert_eq!(structure.style, style);
            let name = &structure.type_name;
            let model = file(
                &files,
                &format!("lib/weather_client/models/{}.rb", snake(name)),
            );
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
                assert!(
                    initialize.contains(&format!("({field}:"))
                        || initialize.contains(&format!(" {field}:")),
                    "{name}.new({field}:)"
                );
                // The YARD type the generated docs state, module-relative.
                let documented = model.lines().any(|line| {
                    line.replace("Runtime::", "")
                        .contains(&format!("@return [{}", member.type_name))
                });
                assert!(documented, "{name}#{field}: {}", member.type_name);
            }
        }
    }
    let output = list.output.as_ref().unwrap();
    assert_eq!(output.type_name, "ListCitiesOutput");
    let cities = output
        .members
        .iter()
        .find(|m| m.member == "cities")
        .unwrap();
    assert_eq!(cities.type_name, "Array<CitySummary>");
}

#[test]
fn construction_reads_an_endpoint_when_none_is_generated() {
    let service = crucible_lang::load_service(&model_dir("weather")).unwrap();
    let surface = surface(&service, &options(None));
    assert_eq!(
        surface.client.construct,
        "Weather::WeatherClient.new(Weather::Runtime::ClientConfig.new(endpoint: endpoint))"
    );
}

#[test]
fn derived_name_collisions_are_refused_not_suffixed() {
    let service =
        crucible_lang::load_service(&model_dir("client-docs-surface-collisions")).unwrap();
    let diagnostics = match emit(&service, &options(None)) {
        Ok(_) => panic!("ListThingsPages collides with list_things' pages enumerator"),
        Err(diagnostics) => diagnostics.to_string(),
    };
    assert!(
        diagnostics.contains("Ruby operation name collision"),
        "{diagnostics}"
    );
}
