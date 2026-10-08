use crucible_smithy::ir::{Service, Shape};
use crucible_smithy::{Audience, Capabilities, Feature, Model, ShapeId, project};

const INTERNAL: &str = include_str!("fixtures/internal.json");

fn id(name: &str) -> ShapeId {
    ShapeId::new("example.audience", name)
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
fn refuses_removed_waiter_output_and_input_output_members() {
    let errors = project(
        &lower(
            include_str!("fixtures/internal-waiter-reference.json"),
            false,
        ),
        Audience::External,
    )
    .unwrap_err();
    assert_eq!(errors.len(), 2, "{errors}");
    assert!(
        errors
            .iter()
            .all(|e| e.message.starts_with("audience.waiter_reference:"))
    );
}

#[test]
fn refuses_endpoint_labels_that_reference_removed_members() {
    let errors = project(
        &lower(
            include_str!("fixtures/internal-endpoint-reference.json"),
            false,
        ),
        Audience::External,
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.starts_with("audience.endpoint_reference:")),
        "{errors}"
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

#[test]
fn refuses_removed_checksum_controls() {
    let errors = project(
        &lower(
            include_str!("fixtures/internal-checksum-reference.json"),
            false,
        ),
        Audience::External,
    )
    .unwrap_err();
    assert_eq!(errors.len(), 2, "{errors}");
    assert!(
        errors
            .iter()
            .all(|e| e.message.starts_with("audience.checksum_reference:"))
    );
}

#[test]
fn refuses_removed_endpoint_discovery_operations() {
    let errors = project(
        &lower(
            include_str!("fixtures/internal-discovery-reference.json"),
            false,
        ),
        Audience::External,
    )
    .unwrap_err();
    assert_eq!(errors.len(), 1, "{errors}");
    assert!(errors.to_string().contains("audience.endpoint_reference:"));
}

#[test]
fn nested_waiter_expressions_check_the_actual_member_scope() {
    let mut full = lower(
        include_str!("fixtures/internal-waiter-reference.json"),
        false,
    );
    use crucible_smithy::ir::{JmesExpression as E, WaiterMatcher};
    // The public output.value is visible while input.privateValue is removed.
    // A same-spelling unrelated member must not change this decision.
    let op = full
        .operations
        .iter_mut()
        .find(|op| op.id == id("Public"))
        .unwrap();
    let waiter = op.waiters.values_mut().next().unwrap();
    waiter.acceptors.truncate(1);
    if let WaiterMatcher::Output { path, .. } = &mut waiter.acceptors[0].matcher {
        path.expression = E::Field {
            name: "value".into(),
        };
    }
    assert!(project(&full, Audience::External).is_ok());
}
