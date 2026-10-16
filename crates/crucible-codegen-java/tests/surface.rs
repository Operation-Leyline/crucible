//! The surface names what the generated package declares.

use std::path::Path;

use crucible_codegen_java::{File, Options, emit, surface};

fn load(name: &str) -> crucible_ir::ir::Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&dir).unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

fn options(endpoint: Option<&str>) -> Options {
    Options {
        group_id: "example".into(),
        artifact_id: "weather-client".into(),
        package_name: "example.weather".into(),
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

/// Whether `code` declares a public method `name` taking parameters.
fn declares(code: &str, name: &str) -> bool {
    code.lines()
        .any(|line| line.trim_start().starts_with("public ") && line.contains(&format!(" {name}(")))
}

#[test]
fn surface_names_what_the_package_declares() {
    for model in ["weather", "client-docs-jvm"] {
        let service = load(model);
        let options = options(Some("https://weather.example"));
        let files = emit(&service, &options).unwrap();
        let surface = surface(&service, &options);
        assert_eq!(surface.language, "java");
        assert_eq!(surface.package.ecosystem, "maven");
        assert_eq!(surface.package.name, "example:weather-client");
        assert_eq!(surface.package.import, "example.weather");
        let name = &surface.client.name;
        let client = source(&files, &format!("/{name}.java"));
        assert!(client.contains(&format!("public final class {name} {{")));
        assert!(client.contains(&format!("public {name}(ClientConfig config)")));
        assert_eq!(surface.operations.len(), service.operations.len());
        for operation in surface.operations.values() {
            assert!(
                declares(&client, &operation.method),
                "{model}: {}",
                operation.method
            );
            let asynchronous = operation.async_method.as_deref().unwrap();
            assert!(declares(&client, asynchronous), "{asynchronous}");
            for pages in operation.paginators.iter().flatten() {
                assert!(declares(&client, pages), "{pages}");
            }
            for structure in [&operation.input, &operation.output].into_iter().flatten() {
                let model = source(&files, &format!("/model/{}.java", structure.type_name));
                assert!(model.contains(&format!("public final class {}", structure.type_name)));
                for member in &structure.members {
                    let declaration = if structure.style == "builder" {
                        format!("public Builder {}({} item)", member.name, member.type_name)
                    } else {
                        format!(" {}() {{", member.name)
                    };
                    let unqualified = model
                        .replace("java.lang.", "")
                        .replace("java.util.", "")
                        .replace("java.time.", "")
                        .replace("java.math.", "")
                        .replace("com.google.gson.", "")
                        .replace("example.weather.model.", "")
                        .replace("example.docs.model.", "");
                    assert!(
                        unqualified.contains(&declaration),
                        "{}.{}: {declaration}",
                        structure.type_name,
                        member.name
                    );
                }
            }
        }
    }
}

#[test]
fn weather_surface_spells_java() {
    let service = load("weather");
    let surface = surface(&service, &options(None));
    assert_eq!(surface.client.name, "WeatherClient");
    assert_eq!(surface.client.construct, "new WeatherClient(config)");
    let configured = crucible_codegen_java::surface(&service, &options(Some("https://x.example")));
    assert_eq!(
        configured.client.construct,
        "new WeatherClient(new ClientConfig())"
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
    assert_eq!(input.type_name, "ListCitiesInput");
    assert_eq!(input.style, "builder");
    assert!(!input.fallible);
    assert_eq!(input.members[0].name, "nextToken");
    assert_eq!(input.members[0].type_name, "String");
    assert_eq!(list.output.as_ref().unwrap().style, "methods");
    assert!(
        surface.operations["example.weather#Ping"]
            .paginators
            .is_none()
    );
}

/// Java refuses method collisions instead of suffixing them, so there is
/// never a surface for a package that does not generate.
#[test]
fn colliding_method_names_are_refused() {
    let service = load("client-docs-surface-collisions");
    assert!(emit(&service, &options(None)).is_err());
}
