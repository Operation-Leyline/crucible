use crucible_codegen_rust::{Options, generate, generate_client};
use crucible_smithy::{Model, ir::Service};
use serde_json::{Value, json};

const STANDARD: &str =
    include_str!("../../crucible-smithy/tests/fixtures/standard-service-errors.json");
const UNKNOWN: &str = "crucible.errors#UnknownOperationException";

fn edited(edit: impl FnOnce(&mut Value)) -> Service {
    let mut model: Value = serde_json::from_str(STANDARD).unwrap();
    edit(&mut model);
    Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap()
}

#[test]
fn the_shared_exception_overrides_the_validation_reason_even_without_operations() {
    let service = edited(|_| {});
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("Some(internal),\n        Some(unknown),"));
    assert!(code.contains("fn unknown()"));
    assert!(code.contains("Box::new(errors::UnknownOperationException"));
    assert!(code.contains("Box::new(errors::InternalServerException"));
    assert!(code.contains("rejection.is_malformed()"));
    assert!(!code.contains("reason: model::ValidationExceptionReason::UnknownOperation"));
}

#[test]
fn a_renamed_shared_exception_is_selected_by_shape_id() {
    let service = edited(|model| {
        model["shapes"]["example.empty#Empty"]["rename"] = json!({UNKNOWN: "NoSuchRouteException"});
    });
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("Box::new(errors::NoSuchRouteException"));
}

#[test]
fn a_custom_404_requires_explicit_selection_and_can_include_request_id() {
    let custom = "example.empty#ResourceNotFoundException";
    let service = edited(|model| {
        let mut shape = model["shapes"]
            .as_object_mut()
            .unwrap()
            .remove(UNKNOWN)
            .unwrap();
        shape["members"]["requestId"] =
            json!({"target":"smithy.api#String", "traits":{"smithy.api#required":{}}});
        model["shapes"][custom] = shape;
        model["shapes"]["example.empty#Empty"]["errors"][2]["target"] = json!(custom);
    });
    let default = generate(&service, &Options::default()).unwrap();
    assert!(default.contains("reason: model::ValidationExceptionReason::UnknownOperation"));
    let code = generate(
        &service,
        &Options {
            unknown_operation_error: Some(custom.parse().unwrap()),
            ..Options::default()
        },
    )
    .unwrap();
    assert!(code.contains("Box::new(errors::ResourceNotFoundException"));
    assert!(code.contains(
        "request_id: ::crucible_server_runtime_rust::__generated::Context::try_current()"
    ));
}

#[test]
fn invalid_unknown_operation_configuration_is_diagnosed_at_generation() {
    let service = edited(|_| {});
    for id in [
        "example.empty#Missing",
        "crucible.errors#ValidationExceptionReason",
    ] {
        let diagnostics = generate(
            &service,
            &Options {
                unknown_operation_error: Some(id.parse().unwrap()),
                ..Options::default()
            },
        )
        .unwrap_err();
        assert_eq!(diagnostics.len(), 1, "{diagnostics}");
        assert!(
            diagnostics
                .to_string()
                .contains("is not an @error structure")
        );
    }
    for (trait_id, value) in [
        ("smithy.api#httpError", json!(400)),
        ("smithy.api#error", json!("server")),
    ] {
        let service = edited(|model| model["shapes"][UNKNOWN]["traits"][trait_id] = value);
        let diagnostics = generate(&service, &Options::default()).unwrap_err();
        assert!(
            diagnostics
                .to_string()
                .contains("client error with HTTP status 404")
        );
    }
    let service = edited(|model| {
        model["shapes"][UNKNOWN]["members"]["detail"] =
            json!({"target":"smithy.api#String", "traits":{"smithy.api#required":{}}});
    });
    let diagnostics = generate(&service, &Options::default()).unwrap_err();
    assert!(diagnostics.to_string().contains("required members besides"));
}

#[test]
fn declared_standard_errors_are_typed_client_responses() {
    let standard: Value = serde_json::from_str(STANDARD).unwrap();
    let mut weather: Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    for (id, shape) in standard["shapes"].as_object().unwrap() {
        if id.starts_with("crucible.errors#") {
            weather["shapes"][id] = shape.clone();
        }
    }
    weather["shapes"]["example.weather#Weather"]["errors"] =
        standard["shapes"]["example.empty#Empty"]["errors"].clone();
    let service = Model::from_json(&weather.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let code = generate_client(&service).unwrap();
    for name in [
        "UnknownOperationException",
        "ValidationException",
        "InternalServerException",
    ] {
        assert!(code.contains(&format!("{name}(errors::{name})")), "{name}");
    }
    assert!(code.contains("Fault::Server,\n        true,\n        false,"));
}
