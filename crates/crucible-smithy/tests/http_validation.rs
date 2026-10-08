use crucible_smithy::Model;

#[test]
fn duplicate_payloads_are_diagnosed_in_input_and_output() {
    let diagnostics = Model::from_json(include_str!("fixtures/duplicate-payloads.json"))
        .unwrap()
        .lower(None)
        .unwrap_err()
        .to_string();
    for structure in ["Input", "Output"] {
        assert!(
            diagnostics.contains(&format!("{structure}$second"))
                && diagnostics
                    .matches("only one member may bind @httpPayload")
                    .count()
                    == 2,
            "{diagnostics}"
        );
    }
}
