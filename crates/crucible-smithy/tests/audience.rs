use crucible_smithy::ir::{Service, Shape};
use crucible_smithy::{Audience, Capabilities, Feature, Model, ShapeId, project};

const INTERNAL: &str = include_str!("fixtures/internal.json");

fn id(name: &str) -> ShapeId {
    ShapeId::new("example.audience", name)
}

#[test]
fn refuses_effective_client_defaults_that_name_removed_enum_values() {
    let full = lower(
        include_str!("fixtures/internal-default-reference.json"),
        false,
    );
    let errors = project(&full, Audience::External).unwrap_err();
    assert_eq!(errors.len(), 2, "{errors}");
    for name in ["VisibleEnum", "VisibleIntEnum"] {
        assert!(
            errors.iter().any(|error| {
                error.shape == id("PublicOutput").with_member(name)
                    && error.message.starts_with("audience.default_reference:")
            }),
            "{errors}"
        );
    }
    assert!(project(&full, Audience::Internal).is_ok());
}

#[test]
fn public_enum_defaults_and_ignored_client_optional_defaults_remain_valid() {
    let mut model: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/internal-default-reference.json")).unwrap();
    for (name, value) in [
        ("VisibleEnum", serde_json::json!("PUBLIC")),
        ("VisibleIntEnum", serde_json::json!(1)),
    ] {
        model["shapes"]["example.audience#PublicOutput"]["members"][name]["traits"]["smithy.api#default"] =
            value;
    }
    // The @input defaults still name private values, but clients ignore them.
    assert!(project(&lower(&model.to_string(), false), Audience::External).is_ok());
    let mut full = lower(
        include_str!("fixtures/internal-default-reference.json"),
        false,
    );
    let Shape::Structure(output) = full.shapes.get_mut(&id("PublicOutput")).unwrap() else {
        panic!("output structure")
    };
    for member in &mut output.members {
        if matches!(member.name.as_str(), "VisibleEnum" | "VisibleIntEnum") {
            member.client_optional = true;
        }
    }
    assert!(project(&full, Audience::External).is_ok());
}

fn lower(json: &str, infrastructure: bool) -> Service {
    let model = Model::from_json(json).unwrap();
    if infrastructure {
        model.lower_infrastructure(None).unwrap()
    } else {
        model
            .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
            .unwrap()
    }
}

#[test]
fn lowers_visibility_once_and_projects_without_mutating_the_server() {
    let full = lower(INTERNAL, false);
    assert!(
        full.operations
            .iter()
            .find(|op| op.id == id("Secret"))
            .unwrap()
            .internal
    );
    assert!(full.shape(&id("SecretInput")).unwrap().internal());
    assert!(full.shape(&id("SecretValue")).unwrap().internal());
    assert!(
        full.shape(&id("PublicInput"))
            .unwrap()
            .members()
            .iter()
            .find(|m| m.name == "privateValue")
            .unwrap()
            .internal
    );
    let external = project(&full, Audience::External).unwrap();
    assert_eq!(external.operations.len(), 1);
    assert_eq!(external.operations[0].errors, [id("PublicFault")]);
    for name in [
        "SecretInput",
        "SecretValue",
        "InternalFault",
        "PrivateDetails",
    ] {
        assert!(external.shape(&id(name)).is_none());
    }
    for name in ["PublicInput", "PublicOutput", "Choice"] {
        assert!(
            external
                .shape(&id(name))
                .unwrap()
                .members()
                .iter()
                .all(|m| !m.internal)
        );
    }
    assert!(external.operations[0].request.headers.is_empty());
    assert_eq!(external.operations[0].request.body, ["value"]);
    assert_eq!(full.operations.len(), 2);
    assert_eq!(full.shape(&id("PublicInput")).unwrap().members().len(), 3);
    let internal = project(&full, Audience::Internal).unwrap();
    assert_eq!(internal.shapes.len(), full.shapes.len());
    assert_eq!(internal.operations[0].errors.len(), 2);
    assert_eq!(Audience::default(), Audience::External);
}

#[test]
fn required_internal_members_in_input_output_and_error_are_all_reported() {
    let full = lower(
        include_str!("fixtures/internal-required-member.json"),
        false,
    );
    let errors = project(&full, Audience::External).unwrap_err();
    assert_eq!(errors.len(), 3, "{errors}");
    for name in ["PublicInput", "PublicOutput", "PublicFault"] {
        assert!(
            errors
                .iter()
                .any(|e| e.shape == id(name).with_member("requiredSecret")
                    && e.message.starts_with("audience.required_member:"))
        );
    }
    assert!(project(&full, Audience::Internal).is_ok());
}

#[test]
fn refuses_external_members_and_operation_roots_targeting_internal_shapes() {
    let errors = project(
        &lower(include_str!("fixtures/internal-target.json"), false),
        Audience::External,
    )
    .unwrap_err();
    assert_eq!(errors.len(), 2, "{errors}");
    assert!(
        errors
            .iter()
            .all(|e| e.message.starts_with("audience.internal_target:"))
    );
}

#[test]
fn refuses_each_removed_paginator_member() {
    let errors = project(
        &lower(
            include_str!("fixtures/internal-paginator-reference.json"),
            false,
        ),
        Audience::External,
    )
    .unwrap_err();
    assert_eq!(errors.len(), 4, "{errors}");
    assert!(
        errors
            .iter()
            .all(|e| e.message.starts_with("audience.paginator_reference:"))
    );
}

#[test]
fn refuses_internal_lifecycle_operations_only_for_infrastructure() {
    let fixture = include_str!("fixtures/internal-resource-lifecycle.json");
    assert!(project(&lower(fixture, false), Audience::External).is_ok());
    let errors = project(&lower(fixture, true), Audience::External).unwrap_err();
    assert_eq!(errors.len(), 1, "{errors}");
    assert_eq!(
        errors.iter().next().unwrap().shape,
        ShapeId::new("example.resources", "Inbox")
    );
    assert!(errors.to_string().contains("audience.resource_lifecycle:"));
}

#[test]
fn refuses_removing_a_collection_element() {
    let errors = project(
        &lower(
            include_str!("fixtures/internal-collection-member.json"),
            false,
        ),
        Audience::External,
    )
    .unwrap_err();
    assert_eq!(errors.len(), 1, "{errors}");
    assert!(errors.to_string().contains("audience.collection_member:"));
}

#[test]
fn internal_resources_and_listings_are_removed() {
    let mut json: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/resources.json")).unwrap();
    json["shapes"]["example.resources#Message"]["traits"]["smithy.api#internal"] =
        serde_json::json!({});
    for name in ["Store", "Scan", "InspectMessage", "Purge"] {
        json["shapes"][format!("example.resources#{name}")]["traits"]["smithy.api#internal"] =
            serde_json::json!({});
    }
    let projected = project(&lower(&json.to_string(), true), Audience::External).unwrap();
    assert_eq!(projected.resources.len(), 1);
    assert!(
        projected
            .resources
            .values()
            .next()
            .unwrap()
            .resources
            .is_empty()
    );
    assert!(
        projected
            .shape(&ShapeId::new("example.resources", "Body"))
            .is_none()
    );
}

#[test]
fn projection_uses_typed_visibility_even_without_raw_internal_traits() {
    let mut full = lower(INTERNAL, false);
    for op in &mut full.operations {
        op.traits.clear();
    }
    for shape in full.shapes.values_mut() {
        if let Shape::Structure(s) | Shape::Union(s) = shape {
            s.traits.clear();
            for member in &mut s.members {
                member.traits.clear();
            }
        }
    }
    let external = project(&full, Audience::External).unwrap();
    assert_eq!(external.operations.len(), 1);
    assert!(external.shape(&id("SecretInput")).is_none());
}

#[test]
fn all_shape_variants_and_enum_members_have_lowered_visibility() {
    let full = lower(include_str!("fixtures/internal-shapes.json"), false);
    for name in [
        "SecretEnum",
        "SecretIntEnum",
        "SecretList",
        "SecretMap",
        "SecretUnion",
    ] {
        assert!(full.shape(&id(name)).unwrap().internal());
    }
    let projected = project(&full, Audience::External).unwrap();
    for name in [
        "SecretEnum",
        "SecretIntEnum",
        "SecretList",
        "SecretMap",
        "SecretUnion",
    ] {
        assert!(projected.shape(&id(name)).is_none());
    }
    let Shape::Enum(value) = projected.shape(&id("VisibleEnum")).unwrap() else {
        panic!()
    };
    assert_eq!(value.values.len(), 1);
    let Shape::IntEnum(value) = projected.shape(&id("VisibleIntEnum")).unwrap() else {
        panic!()
    };
    assert_eq!(value.values.len(), 1);
}

/// The external view of everything the adversarial check reproduced, as
/// JSON: what it must not contain.
fn external_json(model: &serde_json::Value) -> String {
    let full = lower(&model.to_string(), false);
    serde_json::to_string(&project(&full, Audience::External).unwrap()).unwrap()
}

/// A legacy `@enum` value tagged `internal` leaves the typed enum, and the
/// raw `smithy.api#enum` that still lists it (with its documentation) goes.
/// `smithy.api#references` on a public structure naming an internal
/// resource leaks the resource and its identifier member: dropped.
#[test]
fn references_to_an_internal_resource_are_dropped() {
    let model = serde_json::json!({
        "smithy": "2.0",
        "shapes": {
            "example.refs#Api": {
                "type": "service", "version": "1",
                "traits": { "crucible.protocols#json": {} },
                "operations": [{ "target": "example.refs#Get" }],
                "resources": [{ "target": "example.refs#Vault" }]
            },
            "example.refs#Vault": {
                "type": "resource",
                "identifiers": { "vaultSecretId": { "target": "smithy.api#String" } },
                "traits": { "smithy.api#internal": {} }
            },
            "example.refs#Get": {
                "type": "operation",
                "input": { "target": "example.refs#GetInput" },
                "output": { "target": "example.refs#GetOutput" },
                "traits": { "smithy.api#http": { "method": "POST", "uri": "/get" } }
            },
            "example.refs#GetInput": {
                "type": "structure",
                "traits": { "smithy.api#input": {} },
                "members": { "id": { "target": "smithy.api#String" } }
            },
            "example.refs#GetOutput": {
                "type": "structure",
                "traits": {
                    "smithy.api#references": [
                        { "resource": "example.refs#Vault", "ids": { "vaultSecretId": "id" } }
                    ]
                },
                "members": { "id": { "target": "smithy.api#String" } }
            }
        }
    });
    let external = external_json(&model);
    for private in [
        "example.refs#Vault",
        "vaultSecretId",
        "smithy.api#references",
    ] {
        assert!(!external.contains(private), "{private} leaks: {external}");
    }
}

/// An application of a custom trait whose definition is `@internal` goes,
/// as in Smithy's projection; a public custom trait stays.
#[test]
fn applications_of_an_internal_trait_definition_are_dropped() {
    let model = serde_json::json!({
        "smithy": "2.0",
        "shapes": {
            "example.custom#Api": {
                "type": "service", "version": "1",
                "traits": { "crucible.protocols#json": {} },
                "operations": [{ "target": "example.custom#Get" }]
            },
            "example.custom#auditOwner": {
                "type": "string",
                "traits": { "smithy.api#trait": {}, "smithy.api#internal": {} }
            },
            "example.custom#publicHint": {
                "type": "string",
                "traits": { "smithy.api#trait": {} }
            },
            "example.custom#Get": {
                "type": "operation",
                "input": { "target": "example.custom#GetInput" },
                "traits": { "smithy.api#http": { "method": "POST", "uri": "/get" } }
            },
            "example.custom#GetInput": {
                "type": "structure",
                "traits": { "smithy.api#input": {} },
                "members": {
                    "id": {
                        "target": "smithy.api#String",
                        "traits": {
                            "example.custom#auditOwner": "secret-team-alias",
                            "example.custom#publicHint": "shown"
                        }
                    }
                }
            }
        }
    });
    let external = external_json(&model);
    assert!(!external.contains("secret-team-alias"), "{external}");
    assert!(
        !external.contains("example.custom#auditOwner"),
        "{external}"
    );
    assert!(external.contains("example.custom#publicHint"), "{external}");
    // The internal view keeps the application.
    let full = lower(&model.to_string(), false);
    let internal = serde_json::to_string(&project(&full, Audience::Internal).unwrap()).unwrap();
    assert!(internal.contains("secret-team-alias"));
}
