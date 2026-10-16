//! Every client generator with options that name a package: what the
//! all-clients tests emit with.
use crucible_codegen::Client;

pub fn clients() -> Vec<(&'static str, Client)> {
    let endpoint = Some("https://example.test".to_owned());
    vec![
        (
            "rust",
            Client::Rust(crucible_codegen::RustOptions {
                package_name: Some("hostile-sdk".into()),
                package_version: "1.0.0".into(),
                runtime: crucible_codegen::RuntimeDependency::Git {
                    repository: "https://github.com/Operation-Leyline/crucible".into(),
                    revision: "a".repeat(40),
                    version_requirement: ">=0.1.0, <0.2.0".into(),
                },
                metadata: Default::default(),
            }),
        ),
        (
            "ts",
            Client::Ts(crucible_codegen::TsOptions {
                package_name: "hostile".into(),
                package_version: "1.0.0".into(),
                client_name: None,
                endpoint: endpoint.clone(),
                middleware: vec![],
                package_json: None,
                metadata: Default::default(),
            }),
        ),
        (
            "python",
            Client::Python(crucible_codegen::PythonOptions {
                package_name: "hostile".into(),
                module_name: "hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
            }),
        ),
        (
            "go",
            Client::Go(crucible_codegen::GoOptions {
                module_path: "example.test/hostile".into(),
                package_name: "hostile".into(),
                endpoint: endpoint.clone(),
            }),
        ),
        (
            "java",
            Client::Java(crucible_codegen::JavaOptions {
                group_id: "example.hostile".into(),
                artifact_id: "hostile-sdk".into(),
                package_name: "hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
            }),
        ),
        (
            "csharp",
            Client::Csharp(crucible_codegen::CsharpOptions {
                namespace: "Hostile".into(),
                package_id: "Example.Hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
            }),
        ),
        (
            "kotlin",
            Client::Kotlin(crucible_codegen::KotlinOptions {
                group_id: "example.hostile".into(),
                artifact_id: "hostile-sdk".into(),
                package_name: "hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
            }),
        ),
        (
            "swift",
            Client::Swift(crucible_codegen::SwiftOptions {
                package_name: "hostile".into(),
                module_name: "Hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
            }),
        ),
        (
            "dart",
            Client::Dart(crucible_codegen::DartOptions {
                package_name: "hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
            }),
        ),
        (
            "php",
            Client::Php(crucible_codegen::PhpOptions {
                package_name: "example/hostile".into(),
                namespace: "Hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
            }),
        ),
        (
            "ruby",
            Client::Ruby(crucible_codegen::RubyOptions {
                package_name: "hostile".into(),
                module_name: "Hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
            }),
        ),
        (
            "cpp",
            Client::Cpp(crucible_codegen::CppOptions {
                package_name: "hostile".into(),
                namespace: "Hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
            }),
        ),
        (
            "c",
            Client::C(crucible_codegen::COptions {
                package_name: "hostile".into(),
                prefix: "hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
            }),
        ),
        (
            "r",
            Client::R(crucible_codegen::ROptions {
                package_name: "hostile".into(),
                package_version: "1.0.0".into(),
                endpoint,
            }),
        ),
    ]
}
