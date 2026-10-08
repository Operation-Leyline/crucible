// The fixture deprecates Ping; the builder helper below still registers it.
#![allow(deprecated)]
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::IntoResponse;
use crucible_auth::{JwtAuthenticator, McpAuthenticator, test_support::TestTokenIssuer};
use crucible_server_runtime_rust::mcp::{
    AuthorizationServerMetadata, EndpointAdmission, McpConfig,
};
use crucible_server_runtime_rust::{BuildError, Crucible};
use serde_json::{Value, json};
use weather_service::MemoryCities;

const WELL_KNOWN: &str = "/.well-known/oauth-authorization-server";
const PRM: &str = "/.well-known/oauth-protected-resource/mcp";

fn cognito() -> AuthorizationServerMetadata {
    AuthorizationServerMetadata::cognito("https://api.example.com", "auth.example.com")
}

fn service(config: McpConfig) -> Result<Crucible, BuildError> {
    weather_service::builder(Arc::new(MemoryCities::default()))
        .mcp(config)
        .build()
}

async fn get(
    service: &Crucible,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut request = Request::builder().uri(path).method(method);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = service.handle(request.body(Body::empty()).unwrap()).await;
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        to_bytes(body, usize::MAX).await.unwrap().to_vec(),
    )
}

#[tokio::test]
async fn cognito_metadata_advertises_s256_and_provider_endpoints() {
    let service = service(McpConfig::new().authorization_server_metadata(cognito())).unwrap();
    let (status, headers, body) = get(&service, "GET", WELL_KNOWN, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/json");
    assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    let document: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        document,
        json!({
            "issuer": "https://api.example.com",
            "authorization_endpoint": "https://auth.example.com/oauth2/authorize",
            "token_endpoint": "https://auth.example.com/oauth2/token",
            "revocation_endpoint": "https://auth.example.com/oauth2/revoke",
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported":
                ["none", "client_secret_post", "client_secret_basic"],
        })
    );
    // Crucible has no registration endpoint; advertising one would mislead clients.
    assert!(document.get("registration_endpoint").is_none());
}

#[tokio::test]
async fn generic_metadata_carries_optional_members_and_issuer_path() {
    let metadata = AuthorizationServerMetadata::new(
        "https://api.example.com/tenant/",
        "https://idp.example.com/authorize",
        "https://idp.example.com/token",
    )
    .scopes_supported(["weather/read", "weather/write"]);
    let service = service(McpConfig::new().authorization_server_metadata(metadata)).unwrap();
    // RFC 8414 section 3.1 inserts the well-known suffix before the issuer's path.
    let (status, _, body) = get(&service, "GET", &format!("{WELL_KNOWN}/tenant"), &[]).await;
    assert_eq!(status, StatusCode::OK);
    let document: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(document["issuer"], "https://api.example.com/tenant/");
    assert_eq!(
        document["scopes_supported"],
        json!(["weather/read", "weather/write"])
    );
    assert!(document.get("revocation_endpoint").is_none());
    assert_ne!(
        get(&service, "GET", WELL_KNOWN, &[]).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn metadata_is_public_but_the_mcp_endpoint_stays_guarded() {
    let service = service(
        McpConfig::new()
            .authorization_server_metadata(cognito())
            .endpoint_guard(|_: &axum::http::request::Parts| {
                EndpointAdmission::Respond(StatusCode::UNAUTHORIZED.into_response())
            }),
    )
    .unwrap();
    // A foreign Origin is refused on the endpoint but irrelevant for discovery.
    let origin = [("origin", "https://elsewhere.example")];
    assert_eq!(
        get(&service, "GET", WELL_KNOWN, &origin).await.0,
        StatusCode::OK
    );
    assert_eq!(
        get(&service, "POST", "/mcp", &origin).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        get(&service, "POST", "/mcp", &[]).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn metadata_accepts_only_get_and_head() {
    let service = service(McpConfig::new().authorization_server_metadata(cognito())).unwrap();
    assert_eq!(
        get(&service, "HEAD", WELL_KNOWN, &[]).await.0,
        StatusCode::OK
    );
    for method in ["POST", "PUT", "DELETE"] {
        let (status, headers, _) = get(&service, method, WELL_KNOWN, &[]).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
        assert_eq!(headers[header::ALLOW], "GET, HEAD");
    }
}

/// Protected resource metadata (derived from the token verifiers) must send
/// clients to this service's PKCE-advertising metadata, not to Cognito's.
#[tokio::test]
async fn protected_resource_metadata_names_the_service_issuer() {
    let issuer = TestTokenIssuer::es256("https://issuer.example", "key").unwrap();
    let verifier = JwtAuthenticator::with_jwks(
        issuer.access_config(vec!["mcp-client".into()]),
        issuer.jwks(),
    )
    .unwrap();
    let authorization =
        McpAuthenticator::new("https://api.example.com/mcp", vec![verifier], "weather/mcp")
            .unwrap();
    let derived = service(McpConfig::new().authorization(authorization)).unwrap();
    let (_, _, body) = get(&derived, "GET", PRM, &[]).await;
    let token_issuers =
        serde_json::from_slice::<Value>(&body).unwrap()["authorization_servers"].clone();
    assert_ne!(token_issuers, json!(["https://api.example.com"]));

    let verifier = JwtAuthenticator::with_jwks(
        issuer.access_config(vec!["mcp-client".into()]),
        issuer.jwks(),
    )
    .unwrap();
    let authorization =
        McpAuthenticator::new("https://api.example.com/mcp", vec![verifier], "weather/mcp")
            .unwrap();
    let service = service(
        McpConfig::new()
            .authorization(authorization)
            .authorization_server_metadata(cognito()),
    )
    .unwrap();
    let (status, _, body) = get(&service, "GET", PRM, &[]).await;
    assert_eq!(status, StatusCode::OK);
    let document: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        document["authorization_servers"],
        json!(["https://api.example.com"])
    );
    assert_eq!(document["resource"], "https://api.example.com/mcp");
    // The authorization server document stays public next to a guarded endpoint.
    assert_eq!(
        get(&service, "GET", WELL_KNOWN, &[]).await.0,
        StatusCode::OK
    );
    assert_eq!(
        get(&service, "POST", "/mcp", &[]).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn metadata_is_not_served_unless_configured() {
    let service = service(McpConfig::new()).unwrap();
    assert_ne!(
        get(&service, "GET", WELL_KNOWN, &[]).await.0,
        StatusCode::OK
    );
}

#[test]
fn invalid_metadata_fails_service_construction() {
    let invalid = [
        AuthorizationServerMetadata::new(
            "http://api.example.com",
            "https://idp.example.com/authorize",
            "https://idp.example.com/token",
        ),
        AuthorizationServerMetadata::new(
            "https://api.example.com?tenant=1",
            "https://idp.example.com/authorize",
            "https://idp.example.com/token",
        ),
        AuthorizationServerMetadata::new(
            "https://api.example.com",
            "https://idp.example.com/authorize",
            "/token",
        ),
        cognito().revocation_endpoint("http://auth.example.com/oauth2/revoke"),
    ];
    for metadata in invalid {
        let result = service(McpConfig::new().authorization_server_metadata(metadata));
        assert!(
            matches!(result, Err(BuildError::Prepare(message)) if message.contains("authorization server metadata")),
        );
    }
    let collision = service(
        McpConfig::new()
            .path(WELL_KNOWN)
            .authorization_server_metadata(cognito()),
    );
    assert!(matches!(collision, Err(BuildError::Prepare(message)) if message.contains("collides")));
}
