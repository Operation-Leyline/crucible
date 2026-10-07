#![cfg(feature = "all-clients")]

use crucible_codegen::{Client, File};
use crucible_smithy::Model;

#[test]
fn adapters_preserve_every_native_emitters_output() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    assert_eq!(
        Client::Rust.emit(&service).unwrap(),
        vec![File {
            path: "src/lib.rs".into(),
            contents: crucible_codegen_rust::generate_client(&service).unwrap()
        }]
    );
    macro_rules! compare {
        ($variant:ident, $emitter:ident, $options:expr) => {
            let options = $options;
            let expected: Vec<File> = $emitter::emit(&service, &options)
                .unwrap()
                .into_iter()
                .map(|file| File {
                    path: file.path,
                    contents: file.contents,
                })
                .collect();
            let client = Client::$variant(options);
            assert_eq!(client.capabilities(), $emitter::capabilities());
            assert_eq!(client.emit(&service).unwrap(), expected);
        };
    }
    compare!(
        Ts,
        crucible_codegen_ts,
        crucible_codegen::TsOptions {
            package_name: "weather".into(),
            package_version: "1.0.0".into(),
            client_name: None,
            endpoint: Some("https://example.test".into()),
            middleware: vec![],
            package_json: None
        }
    );
    compare!(
        Python,
        crucible_codegen_python,
        crucible_codegen::PythonOptions {
            package_name: "weather".into(),
            module_name: "weather".into(),
            package_version: "1.0.0".into(),
            endpoint: Some("https://example.test".into()),
            content_sha256: true
        }
    );
    compare!(
        Go,
        crucible_codegen_go,
        crucible_codegen::GoOptions {
            module_path: "example.test/weather".into(),
            package_name: "weather".into(),
            endpoint: Some("https://example.test".into()),
            content_sha256: true
        }
    );
    compare!(
        Java,
        crucible_codegen_java,
        crucible_codegen::JavaOptions {
            group_id: "example.weather".into(),
            artifact_id: "weather-sdk".into(),
            package_name: "weather".into(),
            package_version: "1.0.0".into(),
            endpoint: Some("https://example.test".into()),
            content_sha256: true
        }
    );
    compare!(
        Csharp,
        crucible_codegen_csharp,
        crucible_codegen::CsharpOptions {
            namespace: "Weather".into(),
            package_id: "Example.Weather".into(),
            package_version: "1.0.0".into(),
            endpoint: Some("https://example.test".into()),
            content_sha256: true
        }
    );
    compare!(
        Kotlin,
        crucible_codegen_kotlin,
        crucible_codegen::KotlinOptions {
            group_id: "example.weather".into(),
            artifact_id: "weather-sdk".into(),
            package_name: "weather".into(),
            package_version: "1.0.0".into(),
            endpoint: Some("https://example.test".into()),
            content_sha256: true
        }
    );
    compare!(
        Swift,
        crucible_codegen_swift,
        crucible_codegen::SwiftOptions {
            package_name: "weather".into(),
            module_name: "Weather".into(),
            package_version: "1.0.0".into(),
            endpoint: Some("https://example.test".into()),
            content_sha256: true
        }
    );
    compare!(
        Dart,
        crucible_codegen_dart,
        crucible_codegen::DartOptions {
            package_name: "weather".into(),
            package_version: "1.0.0".into(),
            endpoint: Some("https://example.test".into()),
            content_sha256: true
        }
    );
    compare!(
        Php,
        crucible_codegen_php,
        crucible_codegen::PhpOptions {
            package_name: "example/weather".into(),
            namespace: "Weather".into(),
            package_version: "1.0.0".into(),
            endpoint: Some("https://example.test".into()),
            content_sha256: true
        }
    );
    compare!(
        Ruby,
        crucible_codegen_ruby,
        crucible_codegen::RubyOptions {
            package_name: "weather".into(),
            module_name: "Weather".into(),
            package_version: "1.0.0".into(),
            endpoint: Some("https://example.test".into()),
            content_sha256: true
        }
    );
    compare!(
        Cpp,
        crucible_codegen_cpp,
        crucible_codegen::CppOptions {
            package_name: "weather".into(),
            namespace: "Weather".into(),
            package_version: "1.0.0".into(),
            endpoint: Some("https://example.test".into()),
            content_sha256: true
        }
    );
    compare!(
        C,
        crucible_codegen_c,
        crucible_codegen::COptions {
            package_name: "weather".into(),
            prefix: "weather".into(),
            package_version: "1.0.0".into(),
            endpoint: Some("https://example.test".into()),
            content_sha256: true
        }
    );
    compare!(
        R,
        crucible_codegen_r,
        crucible_codegen::ROptions {
            package_name: "weather".into(),
            package_version: "1.0.0".into(),
            endpoint: Some("https://example.test".into()),
            content_sha256: true
        }
    );
}
