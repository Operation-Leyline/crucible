//! The surface names what the generated package declares.

use std::path::Path;

use crucible_codegen_python::{Options, emit, surface};

fn load(name: &str) -> crucible_ir::ir::Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&dir).unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

fn options(endpoint: Option<&str>) -> Options {
    Options {
        package_name: "weather-client".into(),
        module_name: "weather_client".into(),
        package_version: "0.0.0.dev0".into(),
        endpoint: endpoint.map(str::to_owned),
    }
}

fn source(files: &[crucible_codegen_python::File], suffix: &str) -> String {
    files
        .iter()
        .find(|file| file.path.ends_with(suffix))
        .unwrap_or_else(|| panic!("no {suffix}"))
        .contents
        .clone()
}

#[test]
fn surface_names_what_the_package_declares() {
    for model in ["weather", "client-docs"] {
        let service = load(model);
        let options = options(Some("https://weather.example"));
        let files = emit(&service, &options).unwrap();
        let client = source(&files, "client.py");
        let models = source(&files, "models.py");
        let surface = surface(&service, &options);
        assert_eq!(surface.language, "python");
        assert_eq!(surface.package.ecosystem, "pypi");
        assert_eq!(surface.package.name, "weather-client");
        assert_eq!(surface.package.import, "weather_client");
        assert_eq!(
            surface.client.construct,
            format!("{}()", surface.client.name)
        );
        assert!(client.contains(&format!("class {}(SyncClient):", surface.client.name)));
        assert_eq!(surface.operations.len(), service.operations.len());
        for operation in surface.operations.values() {
            assert!(
                client.contains(&format!("    def {}(self, ", operation.method)),
                "{model}: {}",
                operation.method
            );
            assert!(client.contains(&format!("    async def {}(self, ", operation.method)));
            for pages in operation.paginators.iter().flatten() {
                assert!(
                    client.contains(&format!("    def {pages}(self, ")),
                    "{pages}"
                );
            }
            for structure in [&operation.input, &operation.output].into_iter().flatten() {
                assert!(
                    models.contains(&format!("class {}:", structure.type_name)),
                    "{}",
                    structure.type_name
                );
                for member in &structure.members {
                    assert!(
                        models.contains(&format!("    {}: {}", member.name, member.type_name)),
                        "{}.{}",
                        structure.type_name,
                        member.name
                    );
                }
            }
        }
    }
}

#[test]
fn weather_surface_spells_python() {
    let service = load("weather");
    let surface = surface(&service, &options(None));
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(surface.client.construct, "WeatherClient(endpoint=endpoint)");
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "list_cities");
    assert_eq!(list.paginators, Some(vec!["list_cities_pages".to_owned()]));
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, "ListCitiesInput");
    assert_eq!(input.style, "keywords");
    assert_eq!(input.members[0].name, "next_token");
    assert_eq!(input.members[0].type_name, "str");
    assert_eq!(list.output.as_ref().unwrap().style, "properties");
    assert!(
        surface.operations["example.weather#Ping"]
            .paginators
            .is_none()
    );
}

/// Python refuses method collisions instead of suffixing them, so there is
/// never a surface for a package that does not generate.
#[test]
fn colliding_method_names_are_refused() {
    let service = load("client-docs-surface-collisions");
    assert!(emit(&service, &options(None)).is_err());
}
