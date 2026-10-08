use crucible_codegen_rust::{Options, generate};
use crucible_smithy::{Audience, Model, ShapeId};

#[test]
fn description_projection_refusals_keep_ordinary_server_diagnostics() {
    let mut source: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/legacy-enum.json"
    ))
    .unwrap();
    source["shapes"]["example.legacy#Color"]["traits"]["smithy.api#internal"] =
        serde_json::json!({});
    let service = Model::from_json(&source.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_rust::capabilities())
        .unwrap();
    let diagnostics = generate(
        &service,
        &Options {
            explorer: Some(Audience::External),
            validation_error: Some(ShapeId::new("example.legacy", "Color")),
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
