#![cfg(feature = "all-outputs")]
//! Every generator against `tests/models/internal-resource`: whatever the
//! external audience receives names neither internal resource, nor any
//! operation, shape or permission only they hold, and the internal audience
//! receives them. The word appears nowhere else in the model, so a plain
//! search covers documentation, identifiers and registries alike.

use clients::clients;
use crucible_codegen::{Audience, File, Generator, ProvisioningBindings, Server};
use crucible_ir::ir::Service;
use serde_json::{Value, json};

#[path = "common/clients.rs"]
mod clients;
mod common;

fn service() -> Service {
    common::load("internal-resource")
}

/// Each file that names an internal resource, as `path: first such line`.
fn traces(output: &str, files: &[File], problems: &mut Vec<String>) {
    for file in files {
        let lower = file.path.to_lowercase();
        if lower.contains("bulletin") || lower.contains("marginalia") {
            problems.push(format!("{output}: file {}", file.path));
        }
        let found = file.contents.lines().find(|line| {
            let line = line.to_lowercase();
            line.contains("bulletin") || line.contains("marginalia")
        });
        if let Some(line) = found {
            let line: String = line.chars().take(160).collect();
            problems.push(format!("{output}: {}: {line}", file.path));
        }
    }
}

fn names_it(files: &[File]) -> bool {
    files
        .iter()
        .any(|file| file.contents.to_lowercase().contains("bulletin"))
}

fn binding(resource: &str, input: &str) -> Value {
    json!({
        "name": resource,
        "properties": {
            "name": {
                "source": {"shape": format!("example.staff#{input}"), "path": ["name"]},
                "required": true,
                "immutable": false
            }
        },
        "attributes": {
            "id": {
                "source": {
                    "shape": "example.staff#CreateShelfOutput",
                    "path": ["shelf", "id"]
                }
            }
        },
        "cdk": {"resource_type": format!("Custom::{resource}")},
        "terraform": {
            "name": resource.to_lowercase(),
            "synchronous": true,
            "read_after_write_consistent": true,
            "not_found_errors": ["NotFound"]
        }
    })
}

fn shelf_bindings() -> ProvisioningBindings {
    serde_json::from_value(
        json!({"resources": {"example.staff#Shelf": binding("Shelf", "CreateShelfInput")}}),
    )
    .unwrap()
}

/// The outputs beyond the clients that an audience selects: the reference
/// (which the Explorer renders), OpenAPI, JSON Schema, the CLI, Terraform and
/// CDK.
fn generators() -> Vec<(&'static str, Generator)> {
    vec![
        ("docs", Generator::Docs),
        (
            "openapi",
            Generator::OpenApi(crucible_codegen::OpenApiOptions::default()),
        ),
        (
            "json-schema",
            Generator::JsonSchema(crucible_codegen::JsonSchemaOptions {
                shape: "example.staff#ShelfState".into(),
            }),
        ),
        (
            "cli",
            Generator::Cli(crucible_codegen::CliOptions {
                bin_name: "library".into(),
                package_version: "1.0.0".into(),
                endpoint: Some("https://example.test".into()),
                runtime_path: Some("/crucible".into()),
            }),
        ),
        (
            "terraform",
            Generator::Terraform {
                bindings: shelf_bindings(),
                options: crucible_codegen::TerraformOptions {
                    module_path: "example.com/terraform-provider-library".into(),
                    provider_name: "library".into(),
                    provider_address: "registry.terraform.io/example/library".into(),
                },
            },
        ),
        (
            "cdk",
            Generator::Cdk {
                bindings: shelf_bindings(),
                options: serde_json::from_str(include_str!(
                    "../../crucible-codegen-cdk/tests/fixtures/options.json"
                ))
                .unwrap(),
            },
        ),
    ]
}

#[test]
fn no_external_client_names_an_internal_resource() {
    let service = service();
    let mut problems = Vec::new();
    for (language, client) in clients() {
        let files = client
            .emit_for_audience(&service, Audience::External)
            .unwrap_or_else(|error| panic!("{language}: {error:?}"));
        traces(language, &files, &mut problems);
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_internal_client_has_the_internal_resource() {
    let service = service();
    for (language, client) in clients() {
        let files = client
            .emit_for_audience(&service, Audience::Internal)
            .unwrap_or_else(|error| panic!("{language}: {error:?}"));
        assert!(names_it(&files), "{language}");
    }
}

#[test]
fn no_other_external_output_names_it() {
    let service = service();
    let mut problems = Vec::new();
    for (output, generator) in generators() {
        let files = generator
            .emit_for_audience(&service, Audience::External)
            .unwrap_or_else(|error| panic!("{output}: {error:?}"));
        traces(output, &files, &mut problems);
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_internal_reference_openapi_and_cli_have_it() {
    let service = service();
    for (output, generator) in generators() {
        // These select a public resource and a public shape.
        if matches!(output, "terraform" | "cdk" | "json-schema") {
            continue;
        }
        let files = generator
            .emit_for_audience(&service, Audience::Internal)
            .unwrap_or_else(|error| panic!("{output}: {error:?}"));
        assert!(names_it(&files), "{output}");
    }
}

/// The reference and the Explorer read one description: its badge comes from
/// the resource's flag.
#[test]
fn the_internal_reference_badges_the_resource() {
    let files = Generator::Docs
        .emit_for_audience(&service(), Audience::Internal)
        .unwrap();
    let describe = &files
        .iter()
        .find(|file| file.path == "describe.json")
        .unwrap()
        .contents;
    let describe: Value = serde_json::from_str(describe).unwrap();
    assert_eq!(
        describe["resources"]["example.staff#Bulletin"]["internal"],
        true
    );
    assert_eq!(
        describe["resources"]["example.staff#Shelf"]["internal"],
        false
    );
    assert_eq!(
        describe["resources"]["example.staff#Shelf"]["resources"],
        json!(["example.staff#Marginalia"])
    );
    let index = &files
        .iter()
        .find(|file| file.path.ends_with("index.md"))
        .unwrap()
        .contents;
    assert!(index.contains("### Bulletin **Internal**"), "{index}");
    assert!(index.contains("### Shelf\n"), "{index}");
}

/// A JSON Schema root is selected by name, so the root itself is what the
/// audience must refuse.
#[test]
fn an_internal_shape_is_no_external_json_schema_root() {
    let service = service();
    let generator = Generator::JsonSchema(crucible_codegen::JsonSchemaOptions {
        shape: "example.staff#BulletinView".into(),
    });
    assert!(
        generator
            .emit_for_audience(&service, Audience::External)
            .is_err()
    );
    let files = generator
        .emit_for_audience(&service, Audience::Internal)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert!(names_it(&files));
}

#[test]
fn infrastructure_bindings_reach_an_internal_resource_only_in_its_audience() {
    let service = service();
    let bindings: ProvisioningBindings = serde_json::from_value(json!({"resources": {
        "example.staff#Bulletin": {
            "name": "Bulletin",
            "properties": {
                "headline": {
                    "source": {"shape": "example.staff#CreateBulletinInput", "path": ["headline"]},
                    "required": true,
                    "immutable": false
                }
            },
            "attributes": {},
            "cdk": {"resource_type": "Custom::Bulletin"}
        }
    }}))
    .unwrap();
    let generator = Generator::Cdk {
        bindings,
        options: serde_json::from_str(include_str!(
            "../../crucible-codegen-cdk/tests/fixtures/options.json"
        ))
        .unwrap(),
    };
    let refused = generator
        .emit_for_audience(&service, Audience::External)
        .expect_err("the external audience has no such resource")
        .to_string();
    assert!(
        refused.contains("provisioning resource is not bound to this service"),
        "{refused}"
    );
    let files = generator
        .emit_for_audience(&service, Audience::Internal)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert!(names_it(&files));
}

/// Servers always implement the full model; only MCP, which has no internal
/// audience, leaves the resource's operations out.
#[test]
fn the_server_implements_it_and_mcp_does_not_offer_it() {
    let service = service();
    let options = crucible_codegen::RustServerOptions {
        mcp: true,
        ..Default::default()
    };
    let server = Generator::Server(Server::Rust(options));
    let code = &server.emit(&service).unwrap()[0].contents;
    assert!(code.contains("GET_BULLETIN_OPERATION"), "no operation");
    let tools = code.split_once("MCP_TOOLS").unwrap().1;
    assert!(!tools.contains("\"GetBulletin\""), "{tools}");
    assert!(tools.contains("\"GetShelf\""), "{tools}");
}
