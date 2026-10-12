#![cfg(feature = "all-clients")]
//! Model text that the generated languages' lexers read differently: every
//! line break Unicode has, a byte order mark, control characters, comment
//! terminators and interpolation syntax, in documentation, a deprecation
//! message, enum values, a default, a pattern and a JSON name
//! (`fixtures/hostile-text.json`). Each was a compile error somewhere:
//! rustc refuses a bare CR in a doc comment, Go a BOM anywhere but a file's
//! start, C# U+2028 or NEL in a string literal, and `*/` in a member's docs
//! ended PHP's constructor docblock.

use crucible_codegen::{Client, File};
use crucible_smithy::{Capabilities, Feature, Model};

const HOSTILE: &str = include_str!("../../crucible-smithy/tests/fixtures/hostile-text.json");

/// Characters no generated source file carries raw. Each language has an
/// escape for them in a literal, and documentation drops or rewrites them.
fn forbidden(c: char) -> bool {
    matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}')
        || (c.is_control() && !matches!(c, '\n' | '\t'))
}

/// Every feature, so each client checks the model against its own
/// capabilities when it emits.
fn service() -> crucible_smithy::ir::Service {
    Model::from_json(HOSTILE)
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
        .unwrap()
}

fn clients() -> Vec<(&'static str, Client)> {
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
                content_sha256: true,
            }),
        ),
        (
            "go",
            Client::Go(crucible_codegen::GoOptions {
                module_path: "example.test/hostile".into(),
                package_name: "hostile".into(),
                endpoint: endpoint.clone(),
                content_sha256: true,
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
                content_sha256: true,
            }),
        ),
        (
            "csharp",
            Client::Csharp(crucible_codegen::CsharpOptions {
                namespace: "Hostile".into(),
                package_id: "Example.Hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
                content_sha256: true,
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
                content_sha256: true,
            }),
        ),
        (
            "swift",
            Client::Swift(crucible_codegen::SwiftOptions {
                package_name: "hostile".into(),
                module_name: "Hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
                content_sha256: true,
            }),
        ),
        (
            "dart",
            Client::Dart(crucible_codegen::DartOptions {
                package_name: "hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
                content_sha256: true,
            }),
        ),
        (
            "php",
            Client::Php(crucible_codegen::PhpOptions {
                package_name: "example/hostile".into(),
                namespace: "Hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
                content_sha256: true,
            }),
        ),
        (
            "ruby",
            Client::Ruby(crucible_codegen::RubyOptions {
                package_name: "hostile".into(),
                module_name: "Hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
                content_sha256: true,
            }),
        ),
        (
            "cpp",
            Client::Cpp(crucible_codegen::CppOptions {
                package_name: "hostile".into(),
                namespace: "Hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
                content_sha256: true,
            }),
        ),
        (
            "c",
            Client::C(crucible_codegen::COptions {
                package_name: "hostile".into(),
                prefix: "hostile".into(),
                package_version: "1.0.0".into(),
                endpoint: endpoint.clone(),
                content_sha256: true,
            }),
        ),
        (
            "r",
            Client::R(crucible_codegen::ROptions {
                package_name: "hostile".into(),
                package_version: "1.0.0".into(),
                endpoint,
                content_sha256: true,
            }),
        ),
    ]
}

/// Source files: not data (JSON, whose encoder escapes what it must) and
/// not prose (Markdown, read as text).
fn source(file: &File) -> bool {
    !file.path.ends_with(".json") && !file.path.ends_with(".md")
}

/// A comment line in any of the generated languages.
fn comment(line: &str) -> bool {
    let line = line.trim_start();
    ["*", "/*", "//", "#"]
        .iter()
        .any(|marker| line.starts_with(marker))
}

#[test]
fn hostile_model_text_never_reaches_generated_source_raw() {
    let service = service();
    let mut problems = Vec::new();
    for (language, client) in clients() {
        let files = client
            .emit(&service)
            .unwrap_or_else(|error| panic!("{language}: {error:?}"));
        for file in files.iter().filter(|file| source(file)) {
            for (number, line) in file.contents.split('\n').enumerate() {
                let Some(c) = line.chars().find(|c| forbidden(*c)) else {
                    continue;
                };
                // A single-quoted PHP or Ruby literal takes every byte as
                // written, by definition; there it is not an escaping gap.
                let verbatim =
                    (file.path.ends_with(".php") || file.path.ends_with(".rb")) && !comment(line);
                if !verbatim {
                    problems.push(format!(
                        "{language} {}:{}: U+{:04X} in {line:?}",
                        file.path,
                        number + 1,
                        u32::from(c)
                    ));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The model's `*/` (`end*/` in its documentation and deprecation message)
/// never closes a block comment early, in the languages whose documentation
/// is `/** */`.
#[test]
fn hostile_model_text_never_ends_a_block_comment() {
    let service = service();
    let mut problems = Vec::new();
    for (language, client) in clients() {
        if !["ts", "java", "kotlin", "php", "c", "cpp"].contains(&language) {
            continue;
        }
        for file in client
            .emit(&service)
            .unwrap()
            .iter()
            .filter(|file| source(file))
        {
            for (number, line) in file.contents.lines().enumerate() {
                // In a string literal (Kotlin's `@Deprecated("…")`) it ends nothing.
                if comment(line) && line.contains("end*/") {
                    problems.push(format!("{language} {}:{}: {line:?}", file.path, number + 1));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
