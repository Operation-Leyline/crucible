use axum::http::HeaderName;
use crucible_auth::{IssuerConfig, JwtAuthenticator, McpAuthenticator};

fn verifier() -> JwtAuthenticator {
    JwtAuthenticator::new(IssuerConfig::cognito_access(
        "us-east-1",
        "us-east-1_pool",
        vec!["client".into()],
    ))
    .unwrap()
}

#[test]
fn mcp_requires_access_policy_authorization_header_and_service_scope() {
    assert!(McpAuthenticator::new("https://api.example/mcp", vec![], "service/mcp").is_err());
    let id = JwtAuthenticator::new(IssuerConfig::cognito(
        "us-east-1",
        "us-east-1_pool",
        vec!["client".into()],
    ))
    .unwrap();
    assert!(McpAuthenticator::new("https://api.example/mcp", vec![id], "service/mcp").is_err());
    let alternate = verifier().credential_header(HeaderName::from_static("x-access-token"));
    assert!(
        McpAuthenticator::new("https://api.example/mcp", vec![alternate], "service/mcp").is_err()
    );
    for scope in [
        "",
        "openid",
        "service/",
        "/mcp",
        "service/mcp read",
        "service/\"bad",
        "service/\\bad",
    ] {
        assert!(
            McpAuthenticator::new("https://api.example/mcp", vec![verifier()], scope).is_err(),
            "{scope}"
        );
    }
    assert!(
        McpAuthenticator::new("https://api.example/mcp", vec![verifier()], "service/mcp").is_ok()
    );
}
