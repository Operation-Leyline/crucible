//! Typed permissions are declared in `.cru` models only (docs/permissions.md):
//! a Smithy model that uses their traits is refused, and an explicit
//! `@auth([])` still marks an operation public in the IR.

use crucible_smithy::{Error, Model, ShapeId};
use serde_json::{Value, json};

fn model(get_user_traits: Value, extra: Value) -> String {
    let mut shapes = json!({
        "example.leyline#Leyline": {"type": "service", "version": "1", "traits": {"crucible.protocols#json": {}, "smithy.api#httpBearerAuth": {}}, "operations": [{"target": "example.leyline#GetUser"}]},
        "crucible.protocols#json": {"type": "structure", "traits": {"smithy.api#trait": {}, "smithy.api#protocolDefinition": {}}},
        "example.leyline#GetUser": {"type": "operation", "traits": get_user_traits}
    });
    shapes["example.leyline#GetUser"]["traits"]["smithy.api#http"] =
        json!({"method": "GET", "uri": "/user", "code": 200});
    for (id, shape) in extra.as_object().unwrap() {
        shapes[id] = shape.clone();
    }
    json!({"smithy": "2.0", "shapes": shapes}).to_string()
}

#[test]
fn permission_traits_are_refused_in_smithy_models() {
    let source = model(
        json!({"crucible.auth#requiresNone": "the caller's own credential"}),
        json!({
            "example.leyline#Color": {"type": "enum", "traits": {"crucible.auth#permissionsOf": "example.leyline#Org"}, "members": {
                "READ": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "org:read", "crucible.auth#permissionOnly": {}}}
            }}
        }),
    );
    let Error::Unsupported(diagnostics) =
        Model::from_json(&source).unwrap().lower(None).unwrap_err()
    else {
        panic!("unsupported")
    };
    for (shape, name) in [
        (ShapeId::new("example.leyline", "GetUser"), "requiresNone"),
        (ShapeId::new("example.leyline", "Color"), "permissionsOf"),
        (
            ShapeId::new("example.leyline", "Color").with_member("READ"),
            "permissionOnly",
        ),
    ] {
        assert!(
            diagnostics.iter().any(|d| d.shape == shape
                && d.message
                    .starts_with(&format!("@crucible.auth#{name} is not supported"))
                && d.message.contains("declared in .cru models")),
            "missing {name} on {shape}: {diagnostics}"
        );
    }
}

#[test]
fn only_an_explicit_empty_auth_marks_an_operation_public() {
    let source = model(json!({"smithy.api#auth": []}), json!({}));
    let service = Model::from_json(&source).unwrap().lower(None).unwrap();
    assert!(service.operations[0].public);
    let source = model(json!({}), json!({}));
    let service = Model::from_json(&source).unwrap().lower(None).unwrap();
    assert!(!service.operations[0].public);
    // A service with no scheme leaves its operations without one, which is
    // not a declaration that they are public.
    let mut bare: Value = serde_json::from_str(&source).unwrap();
    bare["shapes"]["example.leyline#Leyline"]["traits"]
        .as_object_mut()
        .unwrap()
        .remove("smithy.api#httpBearerAuth");
    let service = Model::from_json(&bare.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    assert!(service.operations[0].auth.is_empty());
    assert!(!service.operations[0].public);
    // The service's own `@auth([])` is inherited.
    bare["shapes"]["example.leyline#Leyline"]["traits"]["smithy.api#auth"] = json!([]);
    let service = Model::from_json(&bare.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    assert!(service.operations[0].public);
}
