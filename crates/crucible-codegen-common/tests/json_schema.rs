use crucible_codegen_common::json_schema::{key, shape, standalone};
use serde_json::Value;

fn load(name: &str) -> crucible_ir::ir::Service {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&dir).unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

#[test]
fn standalone_schemas_are_identical_to_openapi_definitions_except_reference_roots() {
    let service = load("weather");
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
    let service = load("recursion");
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

#[test]
fn standalone_rewrites_schema_references_without_changing_defaults_or_traits() {
    use crucible_ir::{ShapeId, ir::Shape};
    use serde_json::json;

    let mut service = load("weather");
    let id = service
        .operations
        .iter()
        .find(|op| op.name() == "CreateCity")
        .unwrap()
        .input
        .clone()
        .unwrap();
    let document = ShapeId::prelude("Document");
    assert!(service.shapes.contains_key(&document));
    let data = json!({
        "$ref": "#/components/schemas/example.weather.CitySummary",
        "properties": {"nested": {"$ref": "#/components/schemas/example.weather.CitySummary"}},
        "allOf": [{"$ref": "#/components/schemas/example.weather.CitySummary"}]
    });
    let trait_id = ShapeId::new("example.metadata", "schema");
    let Shape::Structure(input) = service.shapes.get_mut(&id).unwrap() else {
        panic!("input structure")
    };
    input.traits.insert(trait_id.clone(), data.clone());
    let member = &mut input.members[0];
    member.target = document;
    member.constraints = Default::default();
    member.default = Some(data.clone());
    member.wire_default = Some(data.clone());
    member.traits.insert(trait_id.clone(), data.clone());
    let name = member.name.clone();

    let schema = standalone(&service, Some(&id));
    for root in [&schema, &schema["$defs"][key(&id)]] {
        assert_eq!(root["x-crucible-traits"][trait_id.to_string()], data);
        assert_eq!(root["properties"][&name]["default"], data);
        assert_eq!(root["properties"][&name]["x-crucible-default"], data);
        assert_eq!(
            root["properties"][&name]["x-crucible-traits"][trait_id.to_string()],
            data
        );
    }
    assert_eq!(
        schema["properties"]["location"]["anyOf"][0]["$ref"],
        "#/$defs/example.weather.Location"
    );
}
