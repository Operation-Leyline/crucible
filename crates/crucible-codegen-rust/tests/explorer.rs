use crucible_codegen_rust::{Options, generate};
use crucible_smithy::{Audience, ShapeId};

mod common;

#[test]
fn description_projection_refusals_keep_ordinary_server_diagnostics() {
    let service = common::load("internal-target");
    let diagnostics = generate(
        &service,
        &Options {
            explorer: Some(Audience::External),
            validation_error: Some(ShapeId::new("example.audience", "SecretInput")),
            ..Options::default()
        },
    )
    .unwrap_err();
    assert!(
        diagnostics
            .to_string()
            .contains("configured validation error")
    );
    assert!(
        diagnostics
            .to_string()
            .contains("audience.internal_target:")
    );
}
