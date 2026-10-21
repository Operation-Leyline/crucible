use crucible_codegen_jsonschema::{Options, generate};
use crucible_ir::{Audience, ShapeId, ir::Service};
use serde_json::{Value, json};

fn load(name: &str) -> Service {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&root).unwrap()
}
fn document(service: &Service, shape: &str) -> Value {
    serde_json::from_str(
        &generate(
            service,
            &Options {
                shape: shape.into(),
            },
        )
        .unwrap(),
    )
    .unwrap()
}
fn validator(schema: &Value) -> jsonschema::Validator {
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(schema)
        .unwrap()
}
#[test]
fn deterministic_golden_and_draft_2020_12_validation() {
    let service = load("json-schema");
    let output = generate(
        &service,
        &Options {
            shape: "RenderPlan".into(),
        },
    )
    .unwrap();
    assert_eq!(
        output,
        generate(
            &service,
            &Options {
                shape: "example.schema#RenderPlan".into()
            }
        )
        .unwrap()
    );
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/render-plan.schema.json");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        std::fs::write(&golden, &output).unwrap();
    }
    assert_eq!(output, std::fs::read_to_string(golden).unwrap());
    let schema: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert!(schema.get("$id").is_none());
    assert!(schema["$defs"].get("crucible.api.Boolean").is_none());
    assert!(schema["$defs"].get("example.schema.Secret").is_none());
    let check = validator(&schema);
    for valid in [
        json!({"seed":0,"source":{"text":"A prompt"}}),
        json!({"seed":2147483647,"source":{"prior":7},"label":null}),
        json!({"seed":1,"source":{"empty":{}},"children":[{"seed":2,"source":{"text":"Next"}}]}),
    ] {
        assert!(
            check.is_valid(&valid),
            "{valid}: {:?}",
            check.validate(&valid)
        );
    }
    for invalid in [
        json!({"source":{"text":"hello"}}),
        json!({"seed":-1,"source":{"text":"hello"}}),
        json!({"seed":2147483648_i64,"source":{"text":"hello"}}),
        json!({"seed":null,"source":{"text":"hello"}}),
        json!({"seed":1,"source":{}}),
        json!({"seed":1,"source":{"text":"hello","prior":1}}),
        json!({"seed":1,"source":{"text":"bad123"}}),
        json!({"seed":1,"source":{"text":""}}),
        json!({"seed":1,"source":{"prior":null}}),
        json!({"seed":1,"source":{"empty":{}},"children":[null]}),
        json!({"seed":1,"source":{"empty":{}},"children":[
            {"seed":1,"source":{"empty":{}}}, {"seed":1,"source":{"empty":{}}},
            {"seed":1,"source":{"empty":{}}}, {"seed":1,"source":{"empty":{}}}
        ]}),
    ] {
        assert!(!check.is_valid(&invalid), "unexpectedly valid: {invalid}");
    }
    assert_eq!(schema["properties"]["retries"]["default"], 2);
}
#[test]
fn recursive_collection_and_all_references_resolve_locally() {
    let schema = document(&load("recursion"), "Node");
    let check = validator(&schema);
    assert!(check.is_valid(&json!({"children":[{"children":[{}]}]})));
    assert!(!check.is_valid(&json!({"children":[1]})));
    assert!(!schema.to_string().contains("#/components/schemas/"));
    assert_eq!(schema["$defs"].as_object().unwrap().len(), 2);
}
#[test]
fn selected_shape_diagnostics_and_audience_projection() {
    let service = load("json-schema");
    for name in ["", "bad name", "example.schema#RenderPlan$seed", "#Bad"] {
        let error = generate(&service, &Options { shape: name.into() })
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid JSON Schema shape"), "{error}");
    }
    let external = crucible_ir::project(&service, Audience::External).unwrap();
    let input = service.operations[0].input.as_ref().unwrap().to_string();
    let external_input = document(&external, &input);
    assert!(external_input["properties"].get("secret").is_none());
    assert!(
        external_input["$defs"]
            .get("example.schema.Secret")
            .is_none()
    );
    let internal_input = document(&service, &input);
    assert!(internal_input["properties"]["secret"].is_object());
    assert!(internal_input["$defs"]["example.schema.Secret"].is_object());

    let error = generate(
        &external,
        &Options {
            shape: "Secret".into(),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("removed by audience projection"), "{error}");
    assert!(
        generate(
            &service,
            &Options {
                shape: "Secret".into()
            }
        )
        .is_ok()
    );
    let error = generate(
        &external,
        &Options {
            shape: "Unknown".into(),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unavailable"), "{error}");
    let mut ambiguous = service.clone();
    let id = ShapeId::new("example.schema", "RenderPlan");
    ambiguous.shapes.insert(
        ShapeId::new("another.namespace", "RenderPlan"),
        service.shape(&id).unwrap().clone(),
    );
    let error = generate(
        &ambiguous,
        &Options {
            shape: "RenderPlan".into(),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("ambiguous"), "{error}");
    assert!(error.contains("another.namespace#RenderPlan"));
    assert!(
        generate(
            &ambiguous,
            &Options {
                shape: id.to_string()
            }
        )
        .is_ok()
    );
}

#[test]
fn every_reachable_shape_exports_in_unrelated_services() {
    for name in ["weather", "recursion", "json-schema"] {
        let service = load(name);
        for id in service.shapes.keys() {
            let schema = document(&service, &id.to_string());
            validator(&schema);
            let expected = crucible_codegen_common::json_schema::standalone(&service, Some(id));
            let mut comparable = schema.clone();
            comparable.as_object_mut().unwrap().remove("$schema");
            assert_eq!(comparable, expected);
        }
    }
}

#[test]
fn unattached_model_exports_are_available_without_operations_or_resources() {
    let service = load("json-schema-unattached");
    assert!(service.operations.is_empty());
    assert!(service.resources.is_empty());
    let schema = document(
        &crucible_ir::project(&service, Audience::External).unwrap(),
        "RenderPlan",
    );
    assert!(validator(&schema).is_valid(&json!({"seed":1,"source":{"empty":{}}})));
    assert_eq!(schema, document(&load("json-schema"), "RenderPlan"));
    assert!(schema["properties"].get("private").is_none());
    assert!(schema["$defs"].get("example.schema.Secret").is_none());
}
