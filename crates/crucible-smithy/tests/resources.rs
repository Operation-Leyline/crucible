use crucible_smithy::{Error, Model, ShapeId};
use serde_json::{Value, json};

const FIXTURE: &str = include_str!("fixtures/resources.json");

fn id(name: &str) -> ShapeId {
    ShapeId::new("example.resources", name)
}

#[test]
fn sdk_lowering_keeps_resource_operations_without_resource_only_shapes() {
    let model = Model::from_json(FIXTURE).unwrap();
    let sdk = model.lower(None).unwrap();
    let infrastructure = model.lower_infrastructure(None).unwrap();
    assert!(sdk.resources.is_empty());
    assert!(sdk.resource_roots.is_empty());
    assert!(sdk.resource_references.is_empty());
    assert!(!sdk.shapes.contains_key(&id("Body")));
    assert!(infrastructure.shapes.contains_key(&id("Body")));
    assert_eq!(
        sdk.operations
            .iter()
            .map(|op| (&op.id, &op.resource))
            .collect::<Vec<_>>(),
        infrastructure
            .operations
            .iter()
            .map(|op| (&op.id, &op.resource))
            .collect::<Vec<_>>()
    );
}

#[test]
fn provisioning_diagnostics_do_not_fail_sdk_lowering() {
    let mut value: Value = serde_json::from_str(FIXTURE).unwrap();
    value["shapes"]["example.resources#ReadOutput"]["members"]["inbox"]["target"] =
        json!("smithy.api#String");
    let model = Model::from_json(&value.to_string()).unwrap();
    assert!(model.lower(None).is_ok());
    assert!(
        model
            .lower_infrastructure(None)
            .unwrap_err()
            .to_string()
            .contains("@nestedProperties must target a structure")
    );
}

fn refused(edit: impl FnOnce(&mut Value)) -> String {
    let mut model: Value = serde_json::from_str(FIXTURE).unwrap();
    edit(&mut model["shapes"]);
    match Model::from_json(&model.to_string())
        .unwrap()
        .lower_infrastructure(None)
    {
        Err(Error::Unsupported(errors)) => errors.to_string(),
        other => panic!("expected collected diagnostics, got {other:?}"),
    }
}

#[test]
fn preserves_resource_graph_and_explicit_lifecycle_slots() {
    let service = Model::from_json(FIXTURE)
        .unwrap()
        .lower_infrastructure(None)
        .unwrap();
    assert_eq!(service.resource_roots, [id("Inbox")]);
    assert_eq!(service.resources.len(), 2);
    let inbox = &service.resources[&id("Inbox")];
    assert_eq!(inbox.lifecycle.create, Some(id("Allocate")));
    assert_eq!(inbox.lifecycle.read, Some(id("Inspect")));
    assert_eq!(inbox.lifecycle.update, Some(id("Change")));
    assert_eq!(inbox.lifecycle.delete, Some(id("Remove")));
    assert_eq!(inbox.resources, [id("Message")]);
    assert_eq!(inbox.docs.as_deref(), Some("A mailbox."));
    assert_eq!(inbox.traits[&id("deployment")]["kind"], "mailbox");
    let message = &service.resources[&id("Message")];
    assert_eq!(message.lifecycle.put, Some(id("Store")));
    assert_eq!(message.lifecycle.list, Some(id("Scan")));
    assert_eq!(message.operations, [id("InspectMessage")]);
    assert_eq!(message.collection_operations, [id("Purge")]);
    // Infrastructure emitters also need resource-only targets.
    assert!(service.shapes.contains_key(&id("Body")));
    assert_eq!(
        service
            .operations
            .iter()
            .find(|op| op.id == id("Store"))
            .unwrap()
            .resource,
        Some(id("Message"))
    );
}

#[test]
fn resolves_member_paths_once_and_preserves_external_references() {
    let service = Model::from_json(FIXTURE)
        .unwrap()
        .lower_infrastructure(None)
        .unwrap();
    let inbox = &service.resources[&id("Inbox")];
    let read = &inbox.bindings[&id("Inspect")];
    assert_eq!(read.input.identifiers["inboxId"], ["id"]);
    assert_eq!(read.output.properties["name"], ["inbox", "name"]);
    assert_eq!(
        inbox.bindings[&id("Allocate")].input.properties["name"],
        ["displayName"]
    );
    let reference = &service.resource_references[&id("InboxData")][0];
    assert!(reference.resolved);
    assert_eq!(reference.identifiers["inboxId"], ["id"]);
    assert!(service.resource_references[&id("InboxId")][0].identifiers["inboxId"].is_empty());
    let external = &service.resource_references[&id("ExternalReference")][0];
    assert!(!external.resolved);
    assert_eq!(external.resource.to_string(), "external.service#Account");
    assert_eq!(external.identifiers["accountId"], ["id"]);
}

#[test]
fn collects_missing_resources_bad_identifiers_and_operation_targets() {
    let errors = refused(|shapes| {
        shapes["example.resources#Service"]["resources"]
            .as_array_mut()
            .unwrap()
            .push(json!({"target": "example.resources#Missing"}));
        shapes["example.resources#Message"]["identifiers"]["messageId"]["target"] =
            json!("smithy.api#Integer");
        shapes["example.resources#Message"]["put"]["target"] = json!("example.resources#Body");
    });
    assert!(
        errors.contains("Missing: resource binding must target"),
        "{errors}"
    );
    assert!(
        errors.contains("identifier `messageId` must target a defined string"),
        "{errors}"
    );
    assert!(
        errors.contains("resource operation binding example.resources#Body"),
        "{errors}"
    );
}

#[test]
fn refuses_resource_cycles_and_missing_parent_identifiers() {
    let errors = refused(|shapes| {
        shapes["example.resources#Message"]["resources"] =
            json!([{"target": "example.resources#Inbox"}]);
        shapes["example.resources#Message"]["identifiers"]
            .as_object_mut()
            .unwrap()
            .remove("inboxId");
    });
    assert!(errors.contains("resource containment cycle"), "{errors}");
    assert!(
        errors.contains("nested resource must retain parent"),
        "{errors}"
    );
}

#[test]
fn refuses_ambiguous_and_malformed_property_bindings() {
    let errors = refused(|shapes| {
        shapes["example.resources#WriteInput"]["members"]["name"] =
            json!({"target": "smithy.api#String"});
        shapes["example.resources#InboxData"]["members"]["name"]["traits"] =
            json!({"smithy.api#property": {"name": "name"}});
        shapes["example.resources#ReadInput"]["members"]["alsoId"] = json!({"target": "smithy.api#String", "traits": {"smithy.api#required": {}, "smithy.api#resourceIdentifier": "inboxId"}});
    });
    assert!(
        errors.contains("ambiguous binding for resource property `name`"),
        "{errors}"
    );
    assert!(
        errors.contains("nested resource properties cannot remap"),
        "{errors}"
    );
    assert!(
        errors.contains("ambiguous explicit binding for resource identifier"),
        "{errors}"
    );
}

#[test]
fn refuses_invalid_reference_mappings_without_rejecting_external_resources() {
    let errors = refused(|shapes| {
        shapes["example.resources#InboxData"]["traits"]["smithy.api#references"][0]["ids"] =
            json!({"unknownId": "missing"});
        shapes["example.resources#InboxId"]["traits"]["smithy.api#references"][0]["ids"] =
            json!({});
    });
    assert!(
        errors.contains("reference maps unknown identifier `unknownId`"),
        "{errors}"
    );
    assert!(
        errors.contains("reference omits identifier `inboxId`"),
        "{errors}"
    );
    assert!(errors.contains("must map to a string member"), "{errors}");
    assert!(
        errors.contains("a string @references must omit ids"),
        "{errors}"
    );
}

#[test]
fn respects_not_property_and_custom_carrier_traits() {
    let mut model: Value = serde_json::from_str(FIXTURE).unwrap();
    model["shapes"]["example.resources#control"] = json!({"type": "structure", "traits": {"smithy.api#trait": {}, "smithy.api#notProperty": {}}});
    model["shapes"]["example.resources#WriteInput"]["members"]["displayName"]["traits"]["example.resources#control"] =
        json!({});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_infrastructure(None)
        .unwrap();
    assert!(
        service.resources[&id("Inbox")].bindings[&id("Allocate")]
            .input
            .properties
            .is_empty()
    );
}

#[test]
fn nested_properties_exclude_adjacent_property_bindings() {
    let mut model: Value = serde_json::from_str(FIXTURE).unwrap();
    model["shapes"]["example.resources#ReadOutput"]["members"]["name"] =
        json!({"target": "smithy.api#String"});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_infrastructure(None)
        .unwrap();
    assert_eq!(
        service.resources[&id("Inbox")].bindings[&id("Inspect")]
            .output
            .properties["name"],
        ["inbox", "name"]
    );
}

#[test]
fn diagnoses_malformed_collection_property_paths() {
    let errors = refused(|shapes| {
        shapes["example.resources#Message"]["collectionOperations"] =
            json!([{"target": "example.resources#Inspect"}]);
        shapes["example.resources#ReadOutput"]["members"]["inbox"]["target"] =
            json!("smithy.api#String");
    });
    assert!(
        errors.contains("@nestedProperties must target a structure"),
        "{errors}"
    );
}

#[test]
fn resolves_explicit_and_inferred_collection_property_paths() {
    for explicit in [false, true] {
        let mut model: Value = serde_json::from_str(FIXTURE).unwrap();
        let shapes = &mut model["shapes"];
        shapes["example.resources#Inbox"]["list"] =
            json!({"target": "example.resources#ListInboxes"});
        shapes["example.resources#ListInboxes"] = json!({"type": "operation", "output": {"target": "example.resources#ListOutput"}, "traits": {"smithy.api#http": {"method": "GET", "uri": "/inboxes"}, "smithy.api#readonly": {}}});
        shapes["example.resources#InboxList"] =
            json!({"type": "list", "member": {"target": "example.resources#InboxData"}});
        shapes["example.resources#ListOutput"] = json!({"type": "structure", "members": {"items": {"target": "example.resources#InboxList"}}});
        if explicit {
            shapes["example.resources#ListOutput"]["members"]["items"]["traits"] =
                json!({"smithy.api#nestedProperties": {}});
        }
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower_infrastructure(None)
            .unwrap();
        let output = &service.resources[&id("Inbox")].bindings[&id("ListInboxes")].output;
        assert!(output.properties.is_empty());
        assert_eq!(output.collections["items"].properties["name"], ["name"]);
    }
}

#[test]
fn string_enums_can_identify_resources_and_carry_references() {
    let mut model: Value = serde_json::from_str(FIXTURE).unwrap();
    model["shapes"]["example.resources#InboxId"]["type"] = json!("enum");
    model["shapes"]["example.resources#InboxId"]["members"] = json!({"PRIMARY": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "primary"}}});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_infrastructure(None)
        .unwrap();
    assert_eq!(
        service.resources[&id("Inbox")].bindings[&id("Inspect")]
            .input
            .identifiers["inboxId"],
        ["id"]
    );
    assert!(service.resource_references[&id("InboxId")][0].resolved);
}

#[test]
fn collection_operations_can_return_one_nested_resource() {
    let mut model: Value = serde_json::from_str(FIXTURE).unwrap();
    model["shapes"]["example.resources#Inbox"]["collectionOperations"] =
        json!([{"target": "example.resources#Inspect"}]);
    model["shapes"]["example.resources#Inbox"]
        .as_object_mut()
        .unwrap()
        .remove("read");
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_infrastructure(None)
        .unwrap();
    assert_eq!(
        service.resources[&id("Inbox")].bindings[&id("Inspect")]
            .output
            .properties["name"],
        ["inbox", "name"]
    );
}
