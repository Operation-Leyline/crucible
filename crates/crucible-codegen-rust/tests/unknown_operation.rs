mod common;

use crucible_codegen_rust::{Options, generate, generate_client};
use crucible_smithy::ir::{Fault, Service, Shape};

const UNKNOWN: &str = "crucible.errors#UnknownOperationException";
const ERRORS: &str =
    "errors [ValidationException, InternalServerException, UnknownOperationException]";

fn standard() -> Service {
    common::load("standard-service-errors")
}

/// The unknown-operation error, for edits to the lowered service.
fn unknown(service: &mut Service) -> &mut crucible_smithy::ir::Structure {
    match service.shapes.get_mut(&UNKNOWN.parse().unwrap()) {
        Some(Shape::Structure(structure)) => structure,
        _ => panic!("{UNKNOWN} is a structure"),
    }
}

#[test]
fn the_shared_exception_overrides_the_validation_reason_even_without_operations() {
    let service = standard();
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
    let mut service = standard();
    common::rename(&mut service, &[(UNKNOWN, "NoSuchRouteException")]);
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("Box::new(errors::NoSuchRouteException"));
}

#[test]
fn a_custom_404_requires_explicit_selection_and_can_include_request_id() {
    let custom = "example.empty#ResourceNotFoundException";
    let service = common::load_with("standard-service-errors", |package| {
        for file in &mut package.files {
            file.text = file.text.replace(
                ERRORS,
                "errors [ValidationException, InternalServerException, ResourceNotFoundException]",
            );
        }
        package.files.push(crucible_lang::SourceFile {
            path: "not_found.cru".into(),
            text: "error ResourceNotFoundException {\n    status 404\n    message: string\n    requestId: string\n}\n"
            .into(),
        });
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
    let service = standard();
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
    for edit in [
        (|service: &mut Service| {
            unknown(service).error.as_mut().unwrap().status = 400;
        }) as fn(&mut Service),
        |service| unknown(service).error.as_mut().unwrap().fault = Fault::Server,
    ] {
        let mut service = standard();
        edit(&mut service);
        let diagnostics = generate(&service, &Options::default()).unwrap_err();
        assert!(
            diagnostics
                .to_string()
                .contains("client error with HTTP status 404")
        );
    }
    let mut service = standard();
    let mut detail = unknown(&mut service).members[0].clone();
    detail.name = "detail".into();
    unknown(&mut service).members.push(detail);
    let diagnostics = generate(&service, &Options::default()).unwrap_err();
    assert!(diagnostics.to_string().contains("required members besides"));
}

#[test]
fn declared_standard_errors_are_typed_client_responses() {
    let service = common::load_with("weather", |package| {
        package.imports.push("crucible.errors".into());
        for file in &mut package.files {
            // The imported package declares its own ValidationError.
            file.text = file
                .text
                .replace("errors [ThrottlingError, ValidationError]", ERRORS)
                .replace("ValidationError", "InvalidRequestError");
        }
    });
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
