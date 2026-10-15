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
fn mcp_requires_access_policy_authorization_header() {
    assert!(McpAuthenticator::new("https://api.example/mcp", vec![]).is_err());
    let id = JwtAuthenticator::new(IssuerConfig::cognito(
        "us-east-1",
        "us-east-1_pool",
        vec!["client".into()],
    ))
    .unwrap();
    assert!(McpAuthenticator::new("https://api.example/mcp", vec![id]).is_err());
    let alternate = verifier().credential_header(HeaderName::from_static("x-access-token"));
    assert!(McpAuthenticator::new("https://api.example/mcp", vec![alternate]).is_err());
    let first_of_many = verifier()
        .credential_headers([
            HeaderName::from_static("x-access-token"),
            HeaderName::from_static("authorization"),
        ])
        .unwrap();
    assert!(McpAuthenticator::new("https://api.example/mcp", vec![first_of_many],).is_err());
    assert!(verifier().credential_headers([]).is_err());
    assert!(McpAuthenticator::new("https://api.example/mcp", vec![verifier()]).is_ok());
}
