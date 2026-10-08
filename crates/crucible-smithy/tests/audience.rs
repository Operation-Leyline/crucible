use crucible_smithy::ir::{Service, Shape};
use crucible_smithy::{Audience, Capabilities, Feature, Model, ShapeId, project};

const INTERNAL: &str = include_str!("fixtures/internal.json");

#[test]
fn legacy_enum_visibility_follows_the_shape_and_preserves_public_values() {
    let source = include_str!("fixtures/legacy-enum.json");
    let full = lower(source, false);
    let color_id = ShapeId::new("example.legacy", "Color");
    let Shape::Enum(color) = full.shape(&color_id).unwrap() else {
        panic!("legacy enum")
    };
    assert!(!color.internal);
    assert!(color.values.iter().all(|value| !value.internal));
    let external = project(&full, Audience::External).unwrap();
    let Shape::Enum(visible) = external.shape(&color_id).unwrap() else {
        panic!("legacy enum")
    };
    assert_eq!(visible.values.len(), 2);

    let mut model: serde_json::Value = serde_json::from_str(source).unwrap();
    model["shapes"]["example.legacy#Color"]["traits"]["smithy.api#internal"] =
        serde_json::json!({});
    let internal = lower(&model.to_string(), false);
    assert!(internal.shape(&color_id).unwrap().internal());
    assert!(project(&internal, Audience::Internal).is_ok());
    let diagnostics = project(&internal, Audience::External).unwrap_err();
    assert_eq!(diagnostics.len(), 2, "{diagnostics}");
    assert!(
        diagnostics
            .iter()
            .all(|d| d.message.starts_with("audience.internal_target:")),
        "{diagnostics}"
    );
}

#[test]
fn projected_request_vectors_check_the_payload_contract() {
    for (target, body, retained) in [
        ("smithy.api#String", "raw public payload", true),
        (
            "example.audience#PrivateDetails",
            r#"{"value":"public"}"#,
            true,
        ),
        (
            "example.audience#PrivateDetails",
            r#"{"hidden":"secret"}"#,
            false,
        ),
    ] {
        let mut model: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/internal-protocol-tests.json")).unwrap();
        model["shapes"]["example.audience#PublicInput"]["members"] = serde_json::json!({
            "value": {"target": target, "traits": {"smithy.api#httpPayload": {}}},
            "privateHeader": {"target": "smithy.api#String", "traits": {
                "smithy.api#internal": {}, "smithy.api#httpHeader": "X-Private"
            }}
        });
        model["shapes"]["example.audience#Public"]["traits"]["smithy.test#httpRequestTests"] = serde_json::json!([{
            "id": "PayloadRequest", "protocol": "crucible.protocols#json",
            "appliesTo": "client", "method": "POST", "uri": "/public",
            "params": {}, "body": body
        }]);
        let external = project(&lower(&model.to_string(), false), Audience::External).unwrap();
        assert_eq!(
            external
                .protocol_tests
                .iter()
                .any(|test| test.id == "PayloadRequest"),
            retained,
            "{target}: {body}"
        );
    }
}

#[test]
fn protocol_vectors_keep_only_representable_params_bindings_and_operations() {
    let full = lower(include_str!("fixtures/internal-protocol-tests.json"), false);
    assert_eq!(full.protocol_tests.len(), 12);
    let external = project(&full, Audience::External).unwrap();
    assert_eq!(
        external
            .protocol_tests
            .iter()
            .map(|test| test.id.as_str())
            .collect::<Vec<_>>(),
        ["PublicRequest", "PublicResponse"]
    );
    assert_eq!(
        project(&full, Audience::Internal).unwrap().protocol_tests,
        full.protocol_tests
    );
}

#[test]
fn cors_keeps_the_full_server_contract_when_client_bindings_are_projected() {
    let full = lower(include_str!("fixtures/internal-cors.json"), false);
    let cors = full.cors.as_ref().unwrap();
    assert_eq!(
        cors.allowed_headers,
        ["content-type", "x-private", "x-secret"]
    );
    assert_eq!(cors.methods, ["DELETE", "POST"]);
    assert_eq!(
        cors.exposed_headers,
        [
            "content-type",
            "x-crucible-error",
            "x-private-error",
            "x-private-output",
            "x-request-id"
        ]
    );
    let external = project(&full, Audience::External).unwrap();
    assert_eq!(external.operations.len(), 1);
    assert!(external.operations[0].request.headers.is_empty());
    assert!(external.operations[0].response.headers.is_empty());
    assert_eq!(external.operations[0].errors, [id("PublicFault")]);
    assert_eq!(external.cors, full.cors);
    assert_eq!(project(&full, Audience::Internal).unwrap().cors, full.cors);
}

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
