use crucible_codegen_common::json_schema::{key, shape, standalone};
use crucible_smithy::Model;
use serde_json::Value;

#[test]
fn standalone_schemas_are_identical_to_openapi_definitions_except_reference_roots() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    for op in &service.operations {
        for id in op.input.iter().chain(&op.output) {
            let schema = standalone(&service, Some(id));
            for (name, definition) in schema["$defs"].as_object().unwrap() {
                let original = service
                    .shapes
                    .iter()
                    .find(|(id, _)| key(id) == *name)
                    .unwrap()
                    .1;
                let expected = shape(&service, original)
                    .to_string()
                    .replace("#/components/schemas/", "#/$defs/");
                assert_eq!(
                    *definition,
                    serde_json::from_str::<Value>(&expected).unwrap()
                );
            }
        }
    }
    let input = standalone(
        &service,
        service
            .operations
            .iter()
            .find(|op| op.name() == "CreateCity")
            .unwrap()
            .input
            .as_ref(),
    );
    assert_eq!(input["required"], serde_json::json!(["name"]));
    assert!(input["properties"]["name"].get("anyOf").is_none());
    for (operation, required) in [
        ("GetCity", serde_json::json!(["cityId"])),
        ("PutCityPhoto", serde_json::json!(["cityId", "photo"])),
    ] {
        let op = service
            .operations
            .iter()
            .find(|op| op.name() == operation)
            .unwrap();
        let schema = standalone(&service, op.input.as_ref());
        assert_eq!(schema["required"], required);
        for name in required.as_array().unwrap() {
            assert!(
                schema["properties"][name.as_str().unwrap()]
                    .get("anyOf")
                    .is_none()
            );
        }
    }
    let unit = standalone(&service, None);
    assert_eq!(unit["type"], "object");
    assert_eq!(unit["maxProperties"], 0);
}

#[test]
fn recursive_refs_stay_local_and_finite() {
    let service = Model::from_json(include_str!("fixtures/recursion.json"))
        .unwrap()
        .lower(None)
        .unwrap();
    let op = service
        .operations
        .iter()
        .find(|op| op.name() == "Get")
        .unwrap();
    let schema = standalone(&service, op.output.as_ref());
    assert!(schema["$defs"]["example.recursion.Node"].is_object());
    assert!(
        schema
            .to_string()
            .contains("#/$defs/example.recursion.Node")
    );
    assert!(schema["$defs"].as_object().unwrap().len() < service.shapes.len());
}
