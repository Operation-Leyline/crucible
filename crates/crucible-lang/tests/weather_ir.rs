//! The weather model exists twice as `.cru`: as the example in
//! `docs/idl/examples/weather` and as the shared test package
//! `tests/models/weather`, which every generator is tested with. Both must
//! compile, and lower to the same operations.

use std::path::Path;

use crucible_lang::{Package, SourceFile, compile};

fn cru_weather() -> (Package, crucible_lang::Compiled) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/idl/examples/weather/model/weather.cru");
    let text = std::fs::read_to_string(&path).expect("weather.cru");
    let package = Package {
        namespace: "example.weather".to_owned(),
        files: vec![SourceFile {
            path: path.display().to_string(),
            text,
        }],
        naming: Default::default(),
        imports: Vec::new(),
        dependencies: Vec::new(),
        dir: None,
        baseline: None,
    };
    let compiled = compile(&package, None);
    (package, compiled)
}

#[test]
fn the_weather_example_compiles_with_its_one_deliberate_warning() {
    let (_, compiled) = cru_weather();
    // The one diagnostic is deliberate: CreateCity's name loosens CityName's
    // length so the e2e tests can produce an output its target refuses.
    let messages: Vec<&str> = compiled
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.as_str())
        .collect();
    assert!(!compiled.has_errors(), "\n{}", compiled.render());
    assert_eq!(messages.len(), 1, "\n{}", compiled.render());
    assert!(
        messages[0].contains("`name`'s `length` loosens"),
        "{messages:?}"
    );
    let service = compiled.service.expect("service");
    assert_eq!(service.operations.len(), 7);
}

#[test]
fn the_weather_test_model_package_compiles() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/weather");
    let service = crucible_lang::load_service(&dir).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(service.operations.len(), 7);
    let (_, example) = cru_weather();
    let names = |service: &crucible_smithy::ir::Service| {
        let mut names: Vec<String> = service
            .operations
            .iter()
            .map(|operation| operation.name().to_owned())
            .collect();
        names.sort();
        names
    };
    assert_eq!(names(&service), names(&example.service.expect("service")));
}
