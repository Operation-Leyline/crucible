use crucible_smithy::Model;

#[test]
fn long_operation_names_remain_valid_smithy_and_are_diagnosed_by_mcp_target() {
    let service = Model::from_json(include_str!("fixtures/mcp-tool-names.json"))
        .unwrap()
        .lower(None)
        .unwrap();
    assert_eq!(service.operations.len(), 2);
    assert!(service.operations.iter().all(|op| op.name().len() > 128));
}

#[test]
fn mcp_streaming_fixture_preserves_all_streaming_operations_in_ir() {
    let service = Model::from_json(include_str!("fixtures/mcp-streaming.json"))
        .unwrap()
        .lower_with_capabilities(
            None,
            &crucible_smithy::Capabilities::new(crucible_smithy::Feature::ALL.iter().copied()),
        )
        .unwrap();
    assert_eq!(service.operations.len(), 5);
    assert!(
        service
            .operations
            .iter()
            .all(|op| op.request.event_stream.is_some()
                || op.response.event_stream.is_some()
                || op.request.payload.as_ref().is_some_and(|p| p.streaming))
    );
}
