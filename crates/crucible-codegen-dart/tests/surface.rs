//! The surface names what the generated package declares.

use crucible_codegen_dart::{Options, emit, surface};
use std::path::{Path, PathBuf};

fn model_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
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

#[test]
fn surface_names_what_the_package_declares() {
    let service = crucible_lang::load_service(&model_dir("weather")).unwrap();
    let options = options(Some("https://weather.example"));
    let files = emit(&service, &options).unwrap();
    let client = file(&files, "lib/src/client.dart");
    let models = file(&files, "lib/src/models.dart");
    let surface = surface(&service, &options);

    assert_eq!(surface.language, "dart");
    assert_eq!(surface.package.ecosystem, "pub");
    assert_eq!(surface.package.name, "weather_client");
    assert_eq!(
        surface.package.import,
        "package:weather_client/weather_client.dart"
    );
    assert!(files.iter().any(|f| f.path == "lib/weather_client.dart"));
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(surface.client.construct, "WeatherClient()");
    assert!(client.contains("final class WeatherClient {"));
    assert!(client.contains("  WeatherClient([ClientConfig? config])"));
    assert_eq!(surface.endpoint.as_deref(), Some("https://weather.example"));

    assert_eq!(surface.operations.len(), service.operations.len());
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "listCities");
    assert_eq!(list.paginators, Some(vec!["listCitiesPages".to_owned()]));
    let get = &surface.operations["example.weather#GetCity"];
    assert_eq!(get.paginators, None);
    for operation in surface.operations.values() {
        assert!(
            client.contains(&format!("> {}(", operation.method)),
            "{}",
            operation.method
        );
        for paginator in operation.paginators.iter().flatten() {
            assert!(
                client.contains(&format!("> {paginator}(models.")),
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
            assert!(models.contains(&format!("final class {name} ")), "{name}");
            assert!(client.contains(&format!("models.{name}")), "{name}");
            for member in &structure.members {
                let field = &member.name;
                // Types are written with the files' import aliases
                // (`core.String`); the surface names them bare.
                let declared = models.lines().any(|line| {
                    let line = line.trim();
                    (line.starts_with("final ") && line.ends_with(&format!(" {field};")))
                        || line.contains(&format!("? get {field} =>"))
                });
                assert!(declared, "{name}.{field}");
                assert!(
                    models.contains(&format!("required this.{field}"))
                        || models.contains(&format!("? {field}, wire.Field<")),
                    "{name}({field}:)"
                );
            }
        }
    }
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, "ListCitiesInput");
    let max = input
        .members
        .iter()
        .find(|m| m.member == "maxResults")
        .unwrap();
    assert_eq!(
        (max.name.as_str(), max.type_name.as_str()),
        ("maxResults", "int")
    );
    let get_output = get.output.as_ref().unwrap();
    assert!(models.contains(&format!("final class {} ", get_output.type_name)));
}

#[test]
fn construction_reads_an_endpoint_when_none_is_generated() {
    let service = crucible_lang::load_service(&model_dir("weather")).unwrap();
    let surface = surface(&service, &options(None));
    assert_eq!(
        surface.client.construct,
        "WeatherClient(ClientConfig(endpoint: endpoint))"
    );
    assert_eq!(surface.endpoint, None);
}

#[test]
fn derived_name_collisions_are_refused_not_suffixed() {
    let service =
        crucible_lang::load_service(&model_dir("client-docs-surface-collisions")).unwrap();
    let diagnostics = match emit(&service, &options(None)) {
        Ok(_) => panic!("ListThingsPages collides with listThings' Pages stream"),
        Err(diagnostics) => diagnostics.to_string(),
    };
    assert!(
        diagnostics.contains("Dart operation name collision"),
        "{diagnostics}"
    );
}
