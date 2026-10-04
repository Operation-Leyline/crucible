use crucible_codegen_go::{Options, emit};
use crucible_smithy::Model;
use std::{fs, path::Path};

#[test]
fn invalid_package_configuration_is_collected() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let errors = emit(
        &service,
        &Options {
            module_path: "bad\nmodule".into(),
            package_name: "type".into(),
            endpoint: None,
            content_sha256: false,
        },
    )
    .err()
    .unwrap();
    assert_eq!(errors.len(), 2, "{errors}");
}

#[test]
fn weather_matches_goldens() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let files = emit(
        &service,
        &Options {
            module_path: "example.com/weather".into(),
            package_name: "weather".into(),
            endpoint: Some("https://weather.example".into()),
            content_sha256: true,
        },
    )
    .unwrap();
    for file in files {
        if file.path == "runtime.go" {
            continue;
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            file.contents,
            "{}",
            file.path
        );
    }
}
