//! Model-declared OAuth scopes, enforced by `ScopeEnforcer` over real HTTP
//! requests carrying verified tokens.
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use crucible_auth::{
    Authenticator, AuthenticatorChain, IssuerConfig, JwtAuthenticator, ScopeEnforcer,
    ScopelessPolicy, TokenKind, test_support::TestTokenIssuer,
};
use crucible_server_runtime_rust::{Crucible, FrameworkError, Operations};
use serde_json::{Value, json};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

include!(concat!(env!("OUT_DIR"), "/scopes.rs"));
use scoped_weather::{Inherits, OptionalWithScopes, OptsOut, Overrides, Public};

fn issuer() -> &'static TestTokenIssuer {
    static ISSUER: OnceLock<TestTokenIssuer> = OnceLock::new();
    ISSUER.get_or_init(|| TestTokenIssuer::new("https://issuer.example", "rsa-1").unwrap())
}

fn operations() -> Operations {
    Operations::builder(&scoped_weather::SERVICE)
        .add::<Inherits, _>(Arc::new(()), |_, _| Box::pin(async { Ok(()) }))
        .add::<Overrides, _>(Arc::new(()), |_, _| Box::pin(async { Ok(()) }))
        .add::<OptsOut, _>(Arc::new(()), |_, _| Box::pin(async { Ok(()) }))
        .add::<Public, _>(Arc::new(()), |_, _| Box::pin(async { Ok(()) }))
        .add::<OptionalWithScopes, _>(Arc::new(()), |_, _| Box::pin(async { Ok(()) }))
        .build()
        .unwrap()
}

fn jwt(config: IssuerConfig) -> Arc<dyn Authenticator> {
    Arc::new(JwtAuthenticator::with_jwks(config, issuer().jwks()).unwrap())
}

fn authenticators() -> Vec<Arc<dyn Authenticator>> {
    vec![
        jwt(issuer().config(vec!["client".into()])),
        jwt(issuer().access_config(vec!["client".into()])),
    ]
}

fn builder() -> crucible_server_runtime_rust::Builder {
    Crucible::builder(operations())
        .defaults()
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .operation_middleware(AuthenticatorChain::new(authenticators()))
}

fn enforcing(enforcer: ScopeEnforcer) -> Crucible {
    builder().interceptor(enforcer).build().unwrap()
}

fn access(scope: Option<&str>) -> String {
    let mut claims = issuer().access_claims("person", "client");
    if let Some(scope) = scope {
        claims["scope"] = json!(scope);
    }
    format!("Bearer {}", issuer().mint(&claims).unwrap())
}

fn id_token() -> String {
    format!(
        "Bearer {}",
        issuer()
            .mint(&issuer().id_claims("person", "client"))
            .unwrap()
    )
}

async fn call(
    service: &Crucible,
    path: &str,
    authorization: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    let mut request = Request::get(path);
    if let Some(authorization) = authorization {
        request = request.header("authorization", authorization);
    }
    let response = service.handle(request.body(Body::empty()).unwrap()).await;
    let (status, headers) = (response.status(), response.headers().clone());
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, headers, body)
}

#[tokio::test]
async fn a_token_holding_the_required_scope_is_served() {
    let service = enforcing(ScopeEnforcer::strict());
    let token = access(Some("weather/read"));
    assert_eq!(
        call(&service, "/inherits", Some(&token)).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn insufficient_scope_is_403_naming_the_operations_scopes() {
    let service = enforcing(ScopeEnforcer::strict());
    // The service default (read) does not satisfy Overrides' own list.
    let token = access(Some("weather/read"));
    let (status, headers, body) = call(&service, "/overrides", Some(&token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["__type"], "ForbiddenException");
    assert_eq!(
        headers["www-authenticate"],
        "Bearer error=\"insufficient_scope\", scope=\"weather/write weather/admin\""
    );
}

#[tokio::test]
async fn an_operation_requires_all_of_its_scopes() {
    let service = enforcing(ScopeEnforcer::strict());
    for (scope, status) in [
        ("weather/write", StatusCode::FORBIDDEN),
        ("weather/admin", StatusCode::FORBIDDEN),
        ("weather/admin weather/write", StatusCode::OK),
        ("weather/read weather/write weather/admin", StatusCode::OK),
    ] {
        let token = access(Some(scope));
        assert_eq!(
            call(&service, "/overrides", Some(&token)).await.0,
            status,
            "{scope}"
        );
    }
}

#[tokio::test]
async fn scopes_match_literally_against_the_space_separated_claim() {
    let service = enforcing(ScopeEnforcer::strict());
    for scope in [
        "WEATHER/READ",
        "weather/read:extra",
        "weather",
        "weather/rea",
        "weather/readx weather/read2",
        "",
    ] {
        let token = access(Some(scope));
        assert_eq!(
            call(&service, "/inherits", Some(&token)).await.0,
            StatusCode::FORBIDDEN,
            "{scope:?}"
        );
    }
    // Runs of whitespace between tokens are separators, not part of a scope.
    let token = access(Some("other   weather/read"));
    assert_eq!(
        call(&service, "/inherits", Some(&token)).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn an_access_token_without_a_scope_claim_is_refused() {
    let service = enforcing(ScopeEnforcer::exempt_id_tokens());
    let token = access(None);
    assert_eq!(
        call(&service, "/inherits", Some(&token)).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn id_tokens_follow_the_configured_scopeless_policy() {
    let token = id_token();
    // Exempting the ID-token kind lets a web session keep working.
    let exempt = enforcing(ScopeEnforcer::exempt_id_tokens());
    assert_eq!(
        call(&exempt, "/inherits", Some(&token)).await.0,
        StatusCode::OK
    );
    // Refusing instead treats a session like any principal without the scope.
    let strict = enforcing(ScopeEnforcer::strict());
    let (status, headers, _) = call(&strict, "/inherits", Some(&token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        headers["www-authenticate"],
        "Bearer error=\"insufficient_scope\", scope=\"weather/read\""
    );
    // Exemption is by token kind: another kind stays checked.
    let other = enforcing(ScopeEnforcer::new(ScopelessPolicy::Exempt(vec![
        TokenKind::ApiKey,
    ])));
    assert_eq!(
        call(&other, "/inherits", Some(&token)).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn the_401_challenge_advertises_the_service_default_scope() {
    let service = enforcing(ScopeEnforcer::strict());
    // Even where the operation opted out of scopes, a client learns the
    // service's default; it is the scope to request when signing in.
    for path in ["/inherits", "/overrides", "/opts-out"] {
        let (status, headers, _) = call(&service, path, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(
            headers["www-authenticate"], "Bearer scope=\"weather/read\"",
            "{path}"
        );
    }
    let (status, headers, _) = call(&service, "/inherits", Some("Bearer garbage")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        headers["www-authenticate"],
        "Bearer error=\"invalid_token\", scope=\"weather/read\""
    );
}

#[tokio::test]
async fn opting_out_and_public_operations_need_no_scope() {
    let service = enforcing(ScopeEnforcer::strict());
    let token = access(None);
    assert_eq!(
        call(&service, "/opts-out", Some(&token)).await.0,
        StatusCode::OK
    );
    let (status, headers, _) = call(&service, "/public", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!headers.contains_key("www-authenticate"));
}

#[tokio::test]
async fn optional_authentication_admits_anonymous_but_checks_a_presented_token() {
    let service = enforcing(ScopeEnforcer::strict());
    assert_eq!(call(&service, "/optional", None).await.0, StatusCode::OK);
    let lacking = access(Some("other"));
    assert_eq!(
        call(&service, "/optional", Some(&lacking)).await.0,
        StatusCode::FORBIDDEN
    );
    let holding = access(Some("weather/read"));
    assert_eq!(
        call(&service, "/optional", Some(&holding)).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn without_the_enforcer_declared_scopes_change_nothing() {
    // Opt-in: the model alone enforces nothing, so existing callers keep working.
    let service = builder().build().unwrap();
    let token = access(None);
    assert_eq!(
        call(&service, "/inherits", Some(&token)).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_scoped_operation_fails_closed_when_no_authenticator_ran() {
    let service = Crucible::builder(operations())
        .defaults()
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .interceptor(ScopeEnforcer::strict())
        .build()
        .unwrap();
    let (status, headers, body) = call(&service, "/inherits", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["__type"], "UnauthenticatedException");
    assert_eq!(headers["www-authenticate"], "Bearer scope=\"weather/read\"");
}

#[tokio::test]
async fn the_refusal_factory_sees_each_framework_error() {
    let forbidden = Arc::new(AtomicUsize::new(0));
    let seen = forbidden.clone();
    let service = enforcing(ScopeEnforcer::strict().refusal_factory(move |error| {
        assert_eq!(error, FrameworkError::Forbidden);
        seen.fetch_add(1, Ordering::SeqCst);
        error.into()
    }));
    let token = access(Some("other"));
    assert_eq!(
        call(&service, "/inherits", Some(&token)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(forbidden.load(Ordering::SeqCst), 1);
}
