use crucible_smithy::{Capabilities, Feature, Model};

#[test]
fn chain_fixture_lowers_effective_scheme_order_and_optional_auth() {
    let model = Model::from_json(include_str!("fixtures/auth-chain.json")).unwrap();
    let service = model
        .lower_with_capabilities(
            None,
            &Capabilities::new([Feature::ApiKeyAuth, Feature::OptionalAuth]),
        )
        .unwrap();
    let operation = |name| {
        service
            .operations
            .iter()
            .find(|op| op.id.name() == name)
            .unwrap()
    };
    assert_eq!(
        operation("Protected")
            .auth
            .iter()
            .map(|id| id.name())
            .collect::<Vec<_>>(),
        ["httpBearerAuth", "httpApiKeyAuth"]
    );
    assert!(operation("Optional").optional_auth);
    assert!(operation("Public").auth.is_empty());
    assert_eq!(operation("BearerOnly").auth[0].name(), "httpBearerAuth");
    assert_eq!(operation("KeyOnly").auth[0].name(), "httpApiKeyAuth");
}
