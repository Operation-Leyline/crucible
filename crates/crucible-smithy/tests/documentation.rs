use crucible_smithy::{Audience, Model, Protocol, ShapeId, project};

fn id(name: &str) -> ShapeId {
    format!("example.docs#{name}").parse().unwrap()
}

#[test]
fn documentation_preserves_protocols_service_errors_resources_and_wire_headers() {
    let model = Model::from_json(include_str!("fixtures/service-docs.json")).unwrap();
    let service = model.lower_documentation(None, None).unwrap();
    assert_eq!(service.protocols, [Protocol::SmithyRpcV2Json]);
    assert_eq!(service.errors, [id("Unavailable")]);
    assert_eq!(
        service.resources[&id("Item")].lifecycle.list,
        Some(id("ListItems"))
    );
    assert!(service.resources[&id("Item")].bindings.is_empty());
    let op = service
        .operations
        .iter()
        .find(|op| op.id == id("WriteItem"))
        .unwrap();
    assert_eq!(op.uri.to_string(), "/service/Catalog/operation/WriteItem");
    assert_eq!(op.protocol_headers["Smithy-Protocol"], "rpc-v2-json");
    assert!(op.request.labels.is_empty());
    let input = service.shape(op.input.as_ref().unwrap()).unwrap();
    let member = input
        .members()
        .into_iter()
        .find(|m| m.name == "item")
        .unwrap();
    assert_eq!(
        service.wire_example(
            member,
            &serde_json::json!({"amount":1234567890123456789_i64}),
            None
        ),
        serde_json::json!({"amount":"1234567890123456789"})
    );
}

#[test]
fn external_projection_removes_nested_internal_example_values() {
    let service = Model::from_json(include_str!("fixtures/service-docs-weather.json"))
        .unwrap()
        .lower_documentation(None, None)
        .unwrap();
    let external = project(&service, Audience::External).unwrap();
    let op = external
        .operations
        .iter()
        .find(|op| op.id.name() == "GetCity")
        .unwrap();
    assert!(
        op.examples[0].output.as_ref().unwrap()["city"]
            .get("secret")
            .is_none()
    );
    let op = service
        .operations
        .iter()
        .find(|op| op.id.name() == "GetCity")
        .unwrap();
    assert_eq!(
        op.examples[0].output.as_ref().unwrap()["city"]["secret"],
        "INTERNAL_EXAMPLE_SECRET"
    );
}

#[test]
fn service_errors_survive_external_projection_without_operations() {
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/service-docs.json")).unwrap();
    value["shapes"]["example.docs#Catalog"]
        .as_object_mut()
        .unwrap()
        .remove("resources");
    let service = Model::from_json(&value.to_string())
        .unwrap()
        .lower_documentation(None, None)
        .unwrap();
    let external = project(&service, Audience::External).unwrap();
    assert_eq!(external.errors, [id("Unavailable")]);
    assert!(external.shape(&id("Unavailable")).is_some());
}

#[test]
fn examples_using_removed_union_or_enum_values_are_diagnosed() {
    let service = Model::from_json(include_str!("fixtures/service-docs-examples-refused.json"))
        .unwrap()
        .lower_documentation(None, None)
        .unwrap();
    let error = project(&service, Audience::External)
        .unwrap_err()
        .to_string();
    assert!(error.contains("audience.example_reference"), "{error}");
    assert!(project(&service, Audience::Internal).is_ok());
}
