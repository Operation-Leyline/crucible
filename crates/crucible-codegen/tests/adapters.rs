#![cfg(feature = "all-clients")]

use crucible_codegen::{Client, File};
use crucible_smithy::Model;

#[test]
fn adapters_preserve_every_native_emitters_output() {
    for fixture in [
        include_str!("../../crucible-smithy/tests/fixtures/weather.json"),
        include_str!("../../crucible-smithy/tests/fixtures/internal.json"),
    ] {
        let full = Model::from_json(fixture).unwrap().lower(None).unwrap();
        let service =
            crucible_smithy::project(&full, crucible_codegen::Audience::External).unwrap();
        let rust_options = crucible_codegen::RustOptions {
            package_name: Some("weather-sdk".into()),
            package_version: "1.0.0".into(),
            runtime: crucible_codegen::RuntimeDependency::Git {
                repository: "https://github.com/Operation-Leyline/crucible".into(),
                revision: "a".repeat(40),
                version_requirement: ">=0.1.0, <0.2.0".into(),
            },
        };
        let expected: Vec<File> =
            crucible_codegen_rust::emit_client_package(&service, &rust_options)
                .unwrap()
                .into_iter()
                .map(|file| File {
                    path: file.path,
                    contents: file.contents,
                })
                .collect();
        assert_eq!(Client::Rust(rust_options).emit(&full).unwrap(), expected);
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
                assert_eq!(client.emit(&full).unwrap(), expected);
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
}
