#![cfg(feature = "all-clients")]

use crucible_codegen::{Client, File};

mod common;

#[test]
fn adapters_preserve_every_native_emitters_output() {
    for fixture in ["weather", "internal"] {
        let full = common::load(fixture);
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
            metadata: Default::default(),
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
        assert_eq!(
            stamped(Client::Rust(rust_options).emit(&full).unwrap()),
            expected
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
                let emitted = client.emit(&full).unwrap();
                // Rust and TypeScript packages gain provenance from the
                // shared layer; the others emit their own.
                let emitted = if stringify!($variant) == "Ts" {
                    stamped(emitted)
                } else {
                    emitted
                };
                assert_eq!(emitted, expected);
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
                package_json: None,
                metadata: Default::default(),
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
            }
        );
        compare!(
            Go,
            crucible_codegen_go,
            crucible_codegen::GoOptions {
                module_path: "example.test/weather".into(),
                package_name: "weather".into(),
                endpoint: Some("https://example.test".into()),
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
            }
        );
        compare!(
            Dart,
            crucible_codegen_dart,
            crucible_codegen::DartOptions {
                package_name: "weather".into(),
                package_version: "1.0.0".into(),
                endpoint: Some("https://example.test".into()),
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
            }
        );
        compare!(
            R,
            crucible_codegen_r,
            crucible_codegen::ROptions {
                package_name: "weather".into(),
                package_version: "1.0.0".into(),
                endpoint: Some("https://example.test".into()),
            }
        );
    }
}

/// The generator's own files from a stamped package, after checking the
/// stamp: `crucible.json` (schema 2) and `crucible-model.json` come last.
fn stamped(mut files: Vec<File>) -> Vec<File> {
    let model = files.pop().expect("a model snapshot");
    let provenance = files.pop().expect("a provenance record");
    assert_eq!(model.path, "crucible-model.json");
    assert_eq!(provenance.path, "crucible.json");
    let record: serde_json::Value = serde_json::from_str(&provenance.contents).unwrap();
    assert_eq!(record["schemaVersion"], 2);
    assert_eq!(
        record["contentDigest"],
        crucible_codegen::provenance::files_digest(&files).unwrap()
    );
    files
}
