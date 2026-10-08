use crucible_auth::{Algorithm, IssuerConfig, JwtAuthenticator, TokenKind};

#[test]
fn insecure_or_ambiguous_trust_configuration_is_rejected() {
    for issuer in [
        "http://example.com",
        "https://user:password@example.com",
        "https://example.com/#fragment",
    ] {
        assert!(
            JwtAuthenticator::new(IssuerConfig::new(
                issuer,
                "https://example.com/jwks",
                vec!["client".into()]
            ))
            .is_err()
        );
    }
    let mut config = IssuerConfig::cognito("us-east-1", "us-east-1_pool", vec!["client".into()]);
    config.algorithms = vec![Algorithm::HS256];
    assert!(JwtAuthenticator::new(config).is_err());
    let mut config = IssuerConfig::cognito("us-east-1", "us-east-1_pool", vec!["client".into()]);
    config.token_rules[0].kind = TokenKind::Access;
    assert!(JwtAuthenticator::new(config).is_err());
    assert!(
        JwtAuthenticator::new(IssuerConfig::cognito("us-east-1", "us-east-1_pool", vec![]))
            .is_err()
    );
}

#[test]
fn cognito_presets_separate_id_and_access_client_claims() {
    let id = IssuerConfig::cognito("us-east-1", "us-east-1_pool", vec!["client".into()]);
    assert_eq!(
        id.issuer,
        "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_pool"
    );
    assert_eq!(id.token_rules[0].kind, TokenKind::Id);
    let access = IssuerConfig::cognito_access("us-east-1", "us-east-1_pool", vec!["client".into()]);
    assert_eq!(access.token_rules[0].kind, TokenKind::Access);
    assert!(JwtAuthenticator::new(access).is_ok());
}
