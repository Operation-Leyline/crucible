#![cfg(feature = "all-clients")]
//! Model text that the generated languages' lexers read differently: every
//! line break Unicode has, a byte order mark, control characters,
//! bidirectional controls, comment terminators, doc-tool tags and
//! interpolation syntax, in documentation, a deprecation message, a waiter's
//! documentation, the service version, enum values, a default, a pattern and
//! a JSON name (`fixtures/hostile-text.json`). Each was a compile error
//! somewhere: rustc refuses a bare CR or a bidirectional control in a doc
//! comment, Go a BOM anywhere but a file's start, C# U+2028 or NEL in a string
//! literal, R a NUL anywhere, and `*/` in a member's docs ended PHP's
//! constructor docblock. Ruby's lexer reads a raw CRLF in a literal as LF,
//! and a line that starts with `@` is a tag to JSDoc, PHPDoc, YARD, Doxygen
//! and roxygen.

use crucible_codegen::{Client, File, Generator, Server};
use crucible_smithy::{Capabilities, Feature, Model};
use serde_json::Value;

const HOSTILE: &str = include_str!("../../crucible-smithy/tests/fixtures/hostile-text.json");

/// Characters no generated source file carries raw. Each language has an
/// escape for them in a literal, and documentation drops or rewrites them.
/// The bidirectional controls are what rustc's deny-by-default
/// `text_direction_codepoint_in_comment` and `_in_literal` lints and
/// GCC/Clang's `-Wbidi-chars` refuse; the marks beside them are as invisible.
fn forbidden(c: char) -> bool {
    matches!(
        c,
        '\u{2028}'
            | '\u{2029}'
            | '\u{feff}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{061c}'
    ) || (c.is_control() && !matches!(c, '\n' | '\t'))
}

/// The fixture as data, for the tests that vary it.
fn fixture() -> Value {
    serde_json::from_str(HOSTILE).unwrap()
}

/// The fixture's documentation: every hostile sequence at once.
fn hostile_doc() -> Value {
    fixture()["shapes"]["example.hostile#Do"]["traits"]["smithy.api#documentation"].clone()
}

/// Every feature, so each client checks the model against its own
/// capabilities when it emits.
fn lower(model: &Value) -> crucible_smithy::ir::Service {
    Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
        .unwrap()
}

fn service() -> crucible_smithy::ir::Service {
    lower(&fixture())
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

/// The fixture as a server lowers it: without the waiter, a client concern.
fn server_service(model: &Value) -> crucible_smithy::ir::Service {
    let mut model = model.clone();
    model["shapes"]["example.hostile#Do"]["traits"]
        .as_object_mut()
        .unwrap()
        .remove("smithy.waiters#waitable");
    lower(&model)
}

/// Every client, the Rust server and the server's operation stubs.
fn outputs(model: &Value) -> Vec<(&'static str, Vec<File>)> {
    let service = lower(model);
    let mut outputs: Vec<(&'static str, Vec<File>)> = clients()
        .into_iter()
        .map(|(language, client)| {
            let files = client
                .emit(&service)
                .unwrap_or_else(|error| panic!("{language}: {error:?}"));
            (language, files)
        })
        .collect();
    let service = server_service(model);
    let server = Server::Rust(Default::default())
        .emit(&service)
        .unwrap_or_else(|error| panic!("rust-server: {error:?}"));
    outputs.push(("rust-server", server));
    let stubs = crucible_codegen_rust::scaffold(&service, &Default::default())
        .into_iter()
        .map(|stub| File {
            path: stub.path().to_string_lossy().replace('\\', "/"),
            contents: stub.contents,
        })
        .collect();
    outputs.push(("rust-scaffold", stubs));
    outputs
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

/// Each line of `files`' source that carries a [`forbidden`] character raw.
fn raw(language: &str, files: &[File], problems: &mut Vec<String>) {
    for file in files.iter().filter(|file| source(file)) {
        for (number, line) in file.contents.split('\n').enumerate() {
            let Some(c) = line.chars().find(|c| forbidden(*c)) else {
                continue;
            };
            // A single-quoted PHP literal takes every byte as written, by
            // definition; there it is not an escaping gap. (Ruby's does not:
            // its lexer reads CRLF as LF even there.)
            let verbatim = file.path.ends_with(".php") && !comment(line);
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

#[test]
fn hostile_model_text_never_reaches_generated_source_raw() {
    let mut problems = Vec::new();
    for (language, files) in outputs(&fixture()) {
        raw(language, &files, &mut problems);
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The service version (`1 cr\rver-lf\n ver-ls…`) stays on the comment line
/// that names it: no generated line starts with what followed a line break.
#[test]
fn hostile_service_version_never_leaves_its_comment() {
    let mut problems = Vec::new();
    for (language, files) in outputs(&fixture()) {
        for file in files.iter().filter(|file| source(file)) {
            for (number, line) in file.contents.lines().enumerate() {
                if line.trim_start().starts_with("ver-") {
                    problems.push(format!("{language} {}:{}: {line:?}", file.path, number + 1));
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
    let mut problems = Vec::new();
    for (language, files) in outputs(&fixture()) {
        if !["ts", "java", "kotlin", "php", "c", "cpp"].contains(&language) {
            continue;
        }
        for file in files.iter().filter(|file| source(file)) {
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

/// The text of a documentation comment line, after its marker.
fn comment_body(line: &str) -> Option<&str> {
    let line = line.trim_start();
    ["#'", "///", "//", "/**", "*", "#"]
        .iter()
        .find_map(|marker| line.strip_prefix(marker))
        .map(str::trim_start)
}

/// A model line that starts with `@` (`@returns injected`) stays prose in
/// the doc tools that read such a line as a tag: JSDoc, Javadoc, KDoc,
/// PHPDoc, YARD, Doxygen and roxygen. JSDoc, PHPDoc and YARD read one inside
/// a code fence (`@throws fenced`) as a tag too; KDoc's lexer knows fences.
#[test]
fn hostile_model_text_never_becomes_a_doc_tag() {
    let mut problems = Vec::new();
    for (language, files) in outputs(&fixture()) {
        if !["ts", "java", "kotlin", "php", "ruby", "c", "cpp", "r"].contains(&language) {
            continue;
        }
        let fences = ["ts", "php", "ruby"].contains(&language);
        for file in files.iter().filter(|file| source(file)) {
            for (number, line) in file.contents.lines().enumerate() {
                if comment_body(line).is_some_and(|body| {
                    body.starts_with("@returns injected")
                        || (fences && body.starts_with("@throws fenced"))
                }) {
                    problems.push(format!("{language} {}:{}: {line:?}", file.path, number + 1));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// An enum value with a NUL: C++ builds it with its length, so the value is
/// not cut short at the NUL; R, whose strings cannot hold one, refuses the
/// model; every other client escapes it.
#[test]
fn nul_in_an_enum_value_is_kept_or_refused() {
    let mut model = fixture();
    model["shapes"]["example.hostile#Kind"]["members"]["NUL"] = serde_json::json!({
        "target": "smithy.api#Unit",
        "traits": {"smithy.api#enumValue": "n\u{0}l"}
    });
    let service = lower(&model);
    let mut problems = Vec::new();
    for (language, client) in clients() {
        match client.emit(&service) {
            Err(error) if language == "r" => assert!(error.to_string().contains("NUL"), "{error}"),
            Ok(_) if language == "r" => {
                problems.push("r: emitted a NUL it cannot represent".into())
            }
            Err(error) => problems.push(format!("{language}: {error}")),
            Ok(files) => {
                raw(language, &files, &mut problems);
                if language == "cpp"
                    && !files
                        .iter()
                        .any(|file| file.contents.contains("std::string(\"n\\000l\", 3)"))
                {
                    problems.push("cpp: the NUL value is not built with its length".into());
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Smithy accepts a shape named `type`; Rust cannot name a type that, and
/// both Rust generators say so instead of emitting `pub type type = …`.
#[test]
fn rust_keyword_type_names_are_diagnosed() {
    let mut model = fixture();
    model["shapes"]["example.hostile#Hostile"]["rename"] =
        serde_json::json!({"example.hostile#Text": "type"});
    let service = lower(&model);
    let client = clients()
        .remove(0)
        .1
        .emit(&service)
        .unwrap_err()
        .to_string();
    assert!(client.contains("`type`"), "{client}");
    let server = Server::Rust(Default::default())
        .emit(&server_service(&model))
        .unwrap_err()
        .to_string();
    assert!(server.contains("`type`"), "{server}");
}

/// Terraform, CDK and the service CLI, from the Terraform fixture with the
/// hostile documentation on its members and service, the hostile version, and
/// the hostile enum as a property's type.
#[cfg(feature = "all-outputs")]
#[test]
fn hostile_model_text_never_reaches_infrastructure_or_cli_source_raw() {
    let mut model: Value = serde_json::from_str(include_str!(
        "../../crucible-codegen-terraform/tests/fixtures/inventory.json"
    ))
    .unwrap();
    let shapes = &mut model["shapes"];
    shapes["example.infra#Inventory"]["version"] =
        fixture()["shapes"]["example.hostile#Hostile"]["version"].clone();
    shapes["example.infra#Inventory"]["traits"]["smithy.api#documentation"] = hostile_doc();
    shapes["example.infra#Kind"] = fixture()["shapes"]["example.hostile#Kind"].clone();
    shapes["example.infra#Widget"]["properties"]["slug"]["target"] = "example.infra#Kind".into();
    for owner in [
        "example.infra#CreateInput",
        "example.infra#UpdateInput",
        "example.infra#WidgetState",
    ] {
        let members = shapes[owner]["members"].as_object_mut().unwrap();
        members["slug"]["target"] = "example.infra#Kind".into();
        for member in members.values_mut() {
            let traits = member
                .as_object_mut()
                .unwrap()
                .entry("traits")
                .or_insert_with(|| serde_json::json!({}));
            traits["smithy.api#documentation"] = hostile_doc();
        }
    }
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_infrastructure(None)
        .unwrap();
    let mut bindings: Value = serde_json::from_str(include_str!(
        "../../crucible-codegen-terraform/tests/fixtures/bindings.json"
    ))
    .unwrap();
    bindings["resources"]["example.infra#Widget"]["cdk"] =
        serde_json::json!({"resource_type": "Custom::Widget"});
    let bindings: crucible_codegen::ProvisioningBindings =
        serde_json::from_value(bindings).unwrap();
    let generators = [
        Generator::Terraform {
            bindings: bindings.clone(),
            options: crucible_codegen::TerraformOptions {
                module_path: "example.com/terraform-provider-inventory".into(),
                provider_name: "inventory".into(),
                provider_address: "registry.terraform.io/example/inventory".into(),
            },
        },
        Generator::Cdk {
            bindings,
            options: serde_json::from_str(include_str!(
                "../../crucible-codegen-cdk/tests/fixtures/options.json"
            ))
            .unwrap(),
        },
    ];
    let mut problems = Vec::new();
    for generator in generators {
        let files = generator
            .emit(&service)
            .unwrap_or_else(|error| panic!("{}: {error:?}", generator.target()));
        raw(generator.target(), &files, &mut problems);
        // CDK's TypeScript documentation is `/** */`; Go's is `//`.
        for file in files.iter().filter(|file| file.path.ends_with(".ts")) {
            for (number, line) in file.contents.lines().enumerate() {
                if comment(line) && line.contains("end*/") {
                    problems.push(format!(
                        "{} {}:{}: {line:?}",
                        generator.target(),
                        file.path,
                        number + 1
                    ));
                }
            }
        }
    }
    let cli = Generator::Cli(crucible_codegen::CliOptions {
        bin_name: "hostile".into(),
        package_version: "1.0.0".into(),
        endpoint: Some("https://example.test".into()),
        runtime_path: "/crucible".into(),
    })
    .emit(&self::service())
    .unwrap_or_else(|error| panic!("cli: {error:?}"));
    raw("cli", &cli, &mut problems);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
