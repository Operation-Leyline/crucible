//! The surface names what the generated package declares.

use crucible_codegen_php::{File, Options, emit, surface};
use std::path::{Path, PathBuf};

fn model_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn options(endpoint: Option<&str>) -> Options {
    Options {
        package_name: "example/weather-client".into(),
        namespace: r"Example\Weather".into(),
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

#[test]
fn surface_names_what_the_package_declares() {
    let service = crucible_lang::load_service(&model_dir("weather")).unwrap();
    let options = options(Some("https://weather.example"));
    let files = emit(&service, &options).unwrap();
    let surface = surface(&service, &options);

    assert_eq!(surface.language, "php");
    assert_eq!(surface.package.ecosystem, "composer");
    assert_eq!(surface.package.name, "example/weather-client");
    assert_eq!(surface.package.import, r"Example\Weather");
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(
        surface.client.construct,
        r"new \Example\Weather\WeatherClient()"
    );
    assert_eq!(surface.endpoint.as_deref(), Some("https://weather.example"));
    let client = file(&files, "src/WeatherClient.php");
    assert!(client.contains(r"namespace Example\Weather;"));
    assert!(client.contains("final class WeatherClient\n"));

    assert_eq!(surface.operations.len(), service.operations.len());
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "listCities");
    assert_eq!(list.paginators, Some(vec!["listCitiesPages".to_owned()]));
    assert_eq!(
        surface.operations["example.weather#GetCity"].paginators,
        None
    );
    for operation in surface.operations.values() {
        assert!(
            client.contains(&format!("    public function {}(", operation.method)),
            "{}",
            operation.method
        );
        for paginator in operation.paginators.iter().flatten() {
            assert!(
                client.contains(&format!("    public function {paginator}(")),
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
            let model = file(&files, &format!("src/{name}.php"));
            assert!(model.contains(&format!("final class {name} ")), "{name}");
            for member in &structure.members {
                let field = &member.name;
                // A promoted, readonly constructor parameter: a named
                // argument and a property at once.
                let promoted = model.lines().any(|line| {
                    let line = line.trim();
                    line.starts_with("public readonly ")
                        && (line.contains(&format!(" ${field} "))
                            || line.ends_with(&format!(" ${field},")))
                });
                assert!(promoted, "{name}::${field}");
                // The docblock names the type the surface reports, qualified.
                let documented = model.lines().any(|line| {
                    let line = line
                        .trim()
                        .replace(r"\Example\Weather\Runtime\", "")
                        .replace(r"\Example\Weather\", "");
                    line.starts_with(&format!("* @param {}", member.type_name))
                        && line.ends_with(&format!(" ${field}"))
                });
                assert!(documented, "{name}::${field}: {}", member.type_name);
            }
        }
    }
    let tags = list
        .input
        .as_ref()
        .unwrap()
        .members
        .iter()
        .find(|m| m.member == "tags")
        .unwrap();
    assert_eq!(
        (tags.name.as_str(), tags.type_name.as_str()),
        ("tags", "list<string>")
    );
}

#[test]
fn construction_reads_an_endpoint_when_none_is_generated() {
    let service = crucible_lang::load_service(&model_dir("weather")).unwrap();
    let surface = surface(&service, &options(None));
    assert_eq!(
        surface.client.construct,
        r"new \Example\Weather\WeatherClient(new \Example\Weather\Runtime\ClientConfig(endpoint: $endpoint))"
    );
}

#[test]
fn derived_name_collisions_are_refused_not_suffixed() {
    let service =
        crucible_lang::load_service(&model_dir("client-docs-surface-collisions")).unwrap();
    let diagnostics = match emit(&service, &options(None)) {
        Ok(_) => panic!("ListThingsPages collides with listThings' Pages generator"),
        Err(diagnostics) => diagnostics.to_string(),
    };
    assert!(
        diagnostics.contains("PHP operation name collision"),
        "{diagnostics}"
    );
}
