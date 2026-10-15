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
