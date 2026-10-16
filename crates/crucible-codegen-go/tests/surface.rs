//! The surface names what the generated module declares.

use std::path::Path;

use crucible_codegen_go::{File, Options, emit, surface};

fn load(name: &str) -> crucible_ir::ir::Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&dir).unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

fn options(endpoint: Option<&str>) -> Options {
    Options {
        module_path: "example.com/weather".into(),
        package_name: "weather".into(),
        endpoint: endpoint.map(str::to_owned),
    }
}

fn source(files: &[File], path: &str) -> String {
    files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("no {path}"))
        .contents
        .clone()
}

#[test]
fn surface_names_what_the_module_declares() {
    for model in ["weather", "client-docs"] {
        let service = load(model);
        let options = options(None);
        let files = emit(&service, &options).unwrap();
        let client = source(&files, "client.go");
        let models = source(&files, "models.go");
        let surface = surface(&service, &options);
        assert_eq!(surface.language, "go");
        assert_eq!(surface.package.ecosystem, "go");
        assert_eq!(surface.package.name, "example.com/weather");
        assert_eq!(surface.package.import, "example.com/weather");
        let name = &surface.client.name;
        assert!(client.contains(&format!("type {name} struct ")));
        assert!(client.contains(&format!("func New{name}(config Config)")));
        assert_eq!(surface.operations.len(), service.operations.len());
        for operation in surface.operations.values() {
            assert!(
                client.contains(&format!(
                    "func (c *{name}) {}(ctx context.Context, ",
                    operation.method
                )),
                "{model}: {}",
                operation.method
            );
            for pages in operation.paginators.iter().flatten() {
                assert!(
                    client.contains(&format!("func (c *{name}) {pages}(input ")),
                    "{pages}"
                );
            }
            for structure in [&operation.input, &operation.output].into_iter().flatten() {
                assert!(
                    models.contains(&format!("type {} struct {{", structure.type_name)),
                    "{}",
                    structure.type_name
                );
                for member in &structure.members {
                    let plain = format!("    {} {}\n", member.name, member.type_name);
                    let optional = format!("    {} Optional[{}]\n", member.name, member.type_name);
                    assert!(
                        models.contains(&plain) || models.contains(&optional),
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
fn weather_surface_spells_go() {
    let service = load("weather");
    let surface = surface(&service, &options(None));
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(
        surface.client.construct,
        "weather.NewWeatherClient(weather.Config{Endpoint: endpoint})"
    );
    let configured = crucible_codegen_go::surface(&service, &options(Some("https://x.example")));
    assert_eq!(
        configured.client.construct,
        "weather.NewWeatherClient(weather.Config{})"
    );
    assert_eq!(configured.endpoint.as_deref(), Some("https://x.example"));
    let list = &surface.operations["example.weather#ListCities"];
    assert_eq!(list.method, "ListCities");
    assert_eq!(list.paginators, Some(vec!["ListCitiesPages".to_owned()]));
    let input = list.input.as_ref().unwrap();
    assert_eq!(input.type_name, "ListCitiesInput");
    assert_eq!(input.style, "fields");
    assert_eq!(input.members[0].name, "NextToken");
    assert_eq!(input.members[0].type_name, "string");
    assert!(
        surface.operations["example.weather#Ping"]
            .paginators
            .is_none()
    );
}

/// Go refuses method collisions instead of suffixing them, so there is
/// never a surface for a module that does not generate.
#[test]
fn colliding_method_names_are_refused() {
    let service = load("client-docs-surface-collisions");
    assert!(emit(&service, &options(None)).is_err());
}
