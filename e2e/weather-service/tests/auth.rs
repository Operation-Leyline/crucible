use axum::{
    Json, Router,
    body::Body,
    http::{HeaderName, Request, StatusCode},
    routing::get,
};
use crucible_auth::{
    AuthError, Authenticator, AuthenticatorChain, IssuerConfig, JwtAuthenticator, Principal,
    test_support::TestTokenIssuer,
};
use crucible_server_runtime_rust::__generated as generated;
use crucible_server_runtime_rust::{
    Context, Crucible, DecodedJob, FrameworkError, Hook, Interceptor, Operation,
    OperationMiddlewareExt, Operations, operation::ServiceInfo,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, OnceLock, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};

include!(concat!(env!("OUT_DIR"), "/auth_chain.rs"));
use auth_weather::{BearerOnly, Protected, Public};

#[tokio::test]
async fn mcp_authenticator_chain_preserves_http_refusals_and_principals() {
    use crucible_server_runtime_rust::mcp::McpConfig;

    for modeled in [false, true] {
        let chain = AuthenticatorChain::new(vec![id_auth()]);
        let chain = if modeled {
            chain.refusal_factory(|_| {
                auth_weather::AccessDeniedException {
                    message: "access denied".into(),
                }
                .into()
            })
        } else {
            chain
        };
        let service = Crucible::builder(operations())
            .defaults()
            .mcp(McpConfig::new())
            .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
            .operation_middleware(chain)
            .interceptor(ObservePrincipal)
            .build()
            .unwrap();
        let token = format!(
            "Bearer {}",
            issuer()
                .mint(&issuer().id_claims("person", "client"))
                .unwrap()
        );
        for (credential, expected, challenge) in [
            (None, StatusCode::UNAUTHORIZED, Some("Bearer")),
            (
                Some("Bearer garbage"),
                StatusCode::UNAUTHORIZED,
                Some("Bearer error=\"invalid_token\""),
            ),
            (Some(token.as_str()), StatusCode::OK, None),
        ] {
            let mut request = Request::post("/mcp").header("content-type", "application/json");
            if let Some(credential) = credential {
                request = request.header("authorization", credential);
            }
            let response = service
                .handle(
                    request
                        .body(Body::from(
                            json!({
                                "jsonrpc":"2.0", "id":1, "method":"tools/call",
                                "params":{"name":"BearerOnly", "arguments":{}}
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await;
            assert_eq!(response.status(), expected, "modeled={modeled}");
            if let Some(challenge) = challenge {
                assert_eq!(response.headers()["www-authenticate"], challenge);
            }
            let body: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            if expected == StatusCode::OK {
                assert_eq!(body["result"]["structuredContent"]["subject"], "person");
            } else {
                assert!(body.get("jsonrpc").is_none(), "{body}");
                if modeled {
                    assert_eq!(body["__type"], "AccessDeniedException");
                }
            }
        }
    }
}

fn issuer() -> &'static TestTokenIssuer {
    static ISSUER: OnceLock<TestTokenIssuer> = OnceLock::new();
    ISSUER.get_or_init(|| TestTokenIssuer::new("https://issuer.example", "rsa-1").unwrap())
}

/// The caller as the operations report it. Each operation has an output of
/// its own with the same two members, so the handlers build whichever they
/// return from the wire form.
fn identity<O: serde::de::DeserializeOwned>() -> O {
    let principal = Context::current().attribute::<Principal>();
    let mut identity = serde_json::Map::new();
    if let Some(principal) = &principal {
        identity.insert("subject".into(), json!(principal.subject));
        identity.insert("kind".into(), json!(format!("{:?}", principal.token_kind)));
    }
    serde_json::from_value(Value::Object(identity)).unwrap()
}

fn operations() -> Operations {
    Operations::builder(&auth_weather::SERVICE)
        .add::<Protected, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .add::<Public, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .add::<BearerOnly, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .build()
        .unwrap()
}

fn service(authenticators: Vec<Arc<dyn Authenticator>>) -> Crucible {
    Crucible::builder(operations())
        .defaults()
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .operation_middleware(AuthenticatorChain::new(authenticators))
        .interceptor(ObservePrincipal)
        .build()
        .unwrap()
}

struct ObservePrincipal;
impl Interceptor for ObservePrincipal {
    fn before<'a>(
        &'a self,
        job: &'a mut DecodedJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        if job.operation().name == "Protected" || job.operation().name == "BearerOnly" {
            assert!(job.context().attribute::<Principal>().is_some());
        }
        Hook::Ready(Ok(()))
    }
}

fn jwt(config: IssuerConfig) -> Arc<dyn Authenticator> {
    let mut config = config;
    config.algorithms = vec![
        crucible_auth::Algorithm::RS256,
        crucible_auth::Algorithm::ES256,
    ];
    Arc::new(JwtAuthenticator::with_jwks(config, issuer().jwks()).unwrap())
}

fn id_auth() -> Arc<dyn Authenticator> {
    jwt(issuer().config(vec!["client".into()]))
}
fn access_auth() -> Arc<dyn Authenticator> {
    jwt(issuer().access_config(vec!["client".into()]))
}

async fn call(
    service: &Crucible,
    path: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut request = Request::get(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = service.handle(request.body(Body::empty()).unwrap()).await;
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, headers, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn id_access_tokens_and_principal_reach_handlers() {
    let service = service(vec![id_auth(), access_auth()]);
    for (claims, kind) in [
        (issuer().id_claims("person", "client"), "Id"),
        (issuer().access_claims("person", "client"), "Access"),
    ] {
        let token = issuer().mint(&claims).unwrap();
        let bearer = format!("bEaReR {token}");
        let (status, _, body) = call(&service, "/protected", &[("authorization", &bearer)]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["subject"], "person");
        assert_eq!(body["kind"], kind);
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/protected", listener.local_addr().unwrap());
    let router = service.into_router();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = reqwest::Client::new();
    for claims in [
        issuer().id_claims("person", "client"),
        issuer().access_claims("person", "client"),
    ] {
        let response = client
            .get(&url)
            .bearer_auth(issuer().mint(&claims).unwrap())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.json::<Value>().await.unwrap()["subject"], "person");
    }
    task.abort();
}

#[tokio::test]
async fn invalid_claims_expiry_and_id_in_access_only_are_refused() {
    let service = service(vec![id_auth(), access_auth()]);
    for (mut claims, name, wrong) in [
        (
            issuer().id_claims("person", "client"),
            "iss",
            json!("https://wrong.example"),
        ),
        (
            issuer().id_claims("person", "client"),
            "aud",
            json!("wrong"),
        ),
        (
            issuer().access_claims("person", "client"),
            "client_id",
            json!("wrong"),
        ),
        (
            issuer().access_claims("person", "client"),
            "token_use",
            json!("id"),
        ),
        (issuer().id_claims("person", "client"), "exp", json!(1)),
        (
            issuer().id_claims("person", "client"),
            "nbf",
            json!(9999999999_u64),
        ),
    ] {
        claims[name] = wrong;
        let bearer = format!("Bearer {}", issuer().mint(&claims).unwrap());
        let (status, headers, body) =
            call(&service, "/protected", &[("authorization", &bearer)]).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{name}");
        assert_eq!(
            headers["www-authenticate"],
            "Bearer error=\"invalid_token\""
        );
        assert_eq!(body["__type"], "UnauthenticatedException");
    }
    let access_only = self::service(vec![access_auth()]);
    let bearer = format!(
        "Bearer {}",
        issuer()
            .mint(&issuer().id_claims("person", "client"))
            .unwrap()
    );
    assert_eq!(
        call(&access_only, "/bearer", &[("authorization", &bearer)])
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn public_operations_ignore_credentials_and_protected_ones_require_them() {
    let service = service(vec![id_auth()]);
    assert_eq!(call(&service, "/public", &[]).await.0, StatusCode::OK);
    assert_eq!(
        call(&service, "/public", &[("authorization", "Bearer garbage")])
            .await
            .0,
        StatusCode::OK
    );
    let (status, headers, _) = call(&service, "/protected", &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(headers["www-authenticate"], "Bearer");
    let bearer = format!(
        "Bearer {}",
        issuer()
            .mint(&issuer().id_claims("person", "client"))
            .unwrap()
    );
    let (status, _, body) = call(&service, "/bearer", &[("authorization", &bearer)]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["subject"], "person");
}

#[tokio::test]
async fn custom_header_requires_bearer_scheme_and_clock_is_injectable() {
    let auth = JwtAuthenticator::with_jwks(issuer().config(vec!["client".into()]), issuer().jwks())
        .unwrap()
        .credential_header(HeaderName::from_static("x-leyline-authorization"))
        .clock(|| UNIX_EPOCH + Duration::from_secs(1000));
    let mut claims = issuer().id_claims("person", "client");
    claims["exp"] = json!(1100);
    claims["nbf"] = json!(990);
    let token = issuer().mint(&claims).unwrap();
    let service = service(vec![Arc::new(auth)]);
    let bearer = format!("Bearer {token}");
    let headers = [
        ("x-leyline-authorization", bearer.as_str()),
        ("authorization", "AWS4-HMAC-SHA256 oac"),
    ];
    assert_eq!(
        call(&service, "/protected", &headers).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &service,
            "/protected",
            &[("x-leyline-authorization", &token)]
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn credential_headers_read_the_first_present_header_in_order() {
    let auth = JwtAuthenticator::with_jwks(issuer().config(vec!["client".into()]), issuer().jwks())
        .unwrap()
        .credential_headers([
            HeaderName::from_static("x-leyline-authorization"),
            HeaderName::from_static("authorization"),
        ])
        .unwrap();
    let service = service(vec![Arc::new(auth)]);
    let token = issuer()
        .mint(&issuer().id_claims("person", "client"))
        .unwrap();
    let bearer = format!("Bearer {token}");
    // The router's header wins over a non-bearer Authorization.
    let routed = [
        ("x-leyline-authorization", bearer.as_str()),
        ("authorization", "AWS4-HMAC-SHA256 oac"),
    ];
    assert_eq!(
        call(&service, "/protected", &routed).await.0,
        StatusCode::OK
    );
    // Unrouted local calls fall back to Authorization.
    assert_eq!(
        call(&service, "/protected", &[("authorization", &bearer)])
            .await
            .0,
        StatusCode::OK
    );
    // The first present header is the only one read.
    let shadowed = [
        ("x-leyline-authorization", "Bearer not.a.token"),
        ("authorization", bearer.as_str()),
    ];
    assert_eq!(
        call(&service, "/protected", &shadowed).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn es256_audience_array_scopes_groups_and_strict_claim_types() {
    let issuer = TestTokenIssuer::es256("https://ec.example", "ec-1").unwrap();
    let mut config = issuer.config(vec!["client".into()]);
    config.algorithms = vec![
        crucible_auth::Algorithm::RS256,
        crucible_auth::Algorithm::ES256,
    ];
    let auth = JwtAuthenticator::with_jwks(config, issuer.jwks()).unwrap();
    let mut claims = issuer.id_claims("person", "client");
    claims["aud"] = json!(["other", "client"]);
    claims["scope"] = json!("read write read");
    claims["cognito:groups"] = json!(["operators", "users"]);
    let principal = auth.verify(&issuer.mint(&claims).unwrap()).await.unwrap();
    assert_eq!(principal.scopes.len(), 2);
    assert!(principal.groups.contains("operators"));
    assert_eq!(principal.claims, claims);
    for (name, value) in [
        ("nbf", json!("1")),
        ("exp", json!("9999999999")),
        ("sub", json!("")),
        ("aud", json!(["client", 1])),
    ] {
        let mut wrong = claims.clone();
        wrong[name] = value;
        assert!(
            auth.verify(&issuer.mint(&wrong).unwrap()).await.is_err(),
            "{name}"
        );
    }
}

struct JwksServer {
    url: String,
    keys: Arc<RwLock<Value>>,
    requests: Arc<AtomicUsize>,
    available: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for JwksServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl JwksServer {
    async fn start() -> Self {
        let keys = Arc::new(RwLock::new(json!({"keys":[]})));
        let requests = Arc::new(AtomicUsize::new(0));
        let available = Arc::new(AtomicBool::new(true));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let data = Arc::clone(&keys);
        let count = Arc::clone(&requests);
        let health = Arc::clone(&available);
        let router = Router::new().route(
            "/jwks",
            get(move || {
                let data = Arc::clone(&data);
                let count = Arc::clone(&count);
                let health = Arc::clone(&health);
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    (
                        if health.load(Ordering::SeqCst) {
                            StatusCode::OK
                        } else {
                            StatusCode::SERVICE_UNAVAILABLE
                        },
                        Json(data.read().unwrap().clone()),
                    )
                }
            }),
        );
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            url,
            keys,
            requests,
            available,
            task,
        }
    }
}

#[tokio::test]
async fn foreign_issuer_tokens_do_not_touch_an_unavailable_provider_in_the_chain() {
    let server = JwksServer::start().await;
    server.available.store(false, Ordering::SeqCst);
    let first = TestTokenIssuer::es256(&server.url, "first").unwrap();
    let second = TestTokenIssuer::es256("https://second.example", "second").unwrap();
    let first_auth = JwtAuthenticator::new(first.config(vec!["client".into()])).unwrap();
    let second_auth =
        JwtAuthenticator::with_jwks(second.config(vec!["client".into()]), second.jwks()).unwrap();
    let service = service(vec![Arc::new(first_auth), Arc::new(second_auth)]);
    let second_bearer = format!(
        "Bearer {}",
        second.mint(&second.id_claims("person", "client")).unwrap()
    );
    let (status, _, body) =
        call(&service, "/protected", &[("authorization", &second_bearer)]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["subject"], "person");
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);

    // A token for this provider must still report its outage, including during cooldown.
    let first_bearer = format!(
        "Bearer {}",
        first.mint(&first.id_claims("person", "client")).unwrap()
    );
    for _ in 0..2 {
        let (status, headers, _) =
            call(&service, "/protected", &[("authorization", &first_bearer)]).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!headers.contains_key("www-authenticate"));
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    assert_eq!(
        call(&service, "/protected", &[("authorization", &second_bearer)])
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn foreign_missing_and_malformed_issuers_do_not_fetch_or_start_jwks_cooldown() {
    let server = JwksServer::start().await;
    let issuer = TestTokenIssuer::es256(&server.url, "first").unwrap();
    *server.keys.write().unwrap() = issuer.jwks();
    let auth = JwtAuthenticator::new(issuer.config(vec!["client".into()])).unwrap();
    let claims = issuer.id_claims("person", "client");
    for value in [json!("https://other.example"), json!(null), json!(1)] {
        let mut wrong = claims.clone();
        wrong["iss"] = value;
        assert!(matches!(
            auth.verify(&issuer.mint(&wrong).unwrap()).await,
            Err(AuthError::Refused)
        ));
    }
    let mut missing = claims.clone();
    missing.as_object_mut().unwrap().remove("iss");
    assert!(matches!(
        auth.verify(&issuer.mint(&missing).unwrap()).await,
        Err(AuthError::Refused)
    ));
    let token = issuer.mint(&claims).unwrap();
    let parts: Vec<_> = token.split('.').collect();
    for payload in ["!", "eyJpc3MiOiI"] {
        let malformed = format!("{}.{payload}.{}", parts[0], parts[2]);
        assert!(matches!(
            auth.verify(&malformed).await,
            Err(AuthError::Refused)
        ));
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
    auth.verify(&token).await.unwrap();
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn unknown_kid_refetch_is_single_flight_rate_limited_and_accepts_rotation() {
    let server = JwksServer::start().await;
    let first = TestTokenIssuer::es256(&server.url, "first").unwrap();
    let next = TestTokenIssuer::es256(&server.url, "next").unwrap();
    *server.keys.write().unwrap() = first.jwks();
    let mut config = first.config(vec!["client".into()]);
    config.min_refresh_interval = Duration::from_millis(50);
    let auth = Arc::new(JwtAuthenticator::new(config).unwrap());
    let old = first.mint(&first.id_claims("person", "client")).unwrap();
    auth.verify(&old).await.unwrap();
    let token = next.mint(&next.id_claims("person", "client")).unwrap();
    for _ in 0..10 {
        assert!(auth.verify(&token).await.is_err());
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    *server.keys.write().unwrap() = next.jwks();
    tokio::time::sleep(Duration::from_millis(60)).await;
    let mut tasks = Vec::new();
    for _ in 0..20 {
        let auth = Arc::clone(&auth);
        let token = token.clone();
        tasks.push(tokio::spawn(
            async move { auth.verify(&token).await.unwrap() },
        ));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
    assert!(
        auth.verify(&old).await.is_err(),
        "removed key is not retained after successful rotation"
    );
}

#[tokio::test]
async fn a_forged_kid_cannot_hold_a_rotated_key_off_for_the_refresh_interval() {
    let server = JwksServer::start().await;
    let first = TestTokenIssuer::es256(&server.url, "first").unwrap();
    let next = TestTokenIssuer::es256(&server.url, "next").unwrap();
    let mut forger = TestTokenIssuer::es256(&server.url, "forged").unwrap();
    *server.keys.write().unwrap() = first.jwks();
    // The default 30 s interval: the floor, not the interval, must decide.
    let auth = JwtAuthenticator::new(first.config(vec!["client".into()])).unwrap();
    auth.verify(&first.mint(&first.id_claims("person", "client")).unwrap())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    // An unverified token with a kid the provider never published fetches once...
    let forged = forger.mint(&forger.id_claims("person", "client")).unwrap();
    assert!(matches!(
        auth.verify(&forged).await,
        Err(AuthError::Refused)
    ));
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
    // ...and the same kid again fetches nothing for the whole interval.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert!(matches!(
        auth.verify(&forged).await,
        Err(AuthError::Refused)
    ));
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
    // Another unseen kid may fetch once the floor has passed: about one
    // fetch a second is all random kids can cause.
    forger.kid = "forged-again".into();
    let forged = forger.mint(&forger.id_claims("person", "client")).unwrap();
    assert!(matches!(
        auth.verify(&forged).await,
        Err(AuthError::Refused)
    ));
    assert_eq!(server.requests.load(Ordering::SeqCst), 3);
    // The provider rotates well inside the 30 s interval the forged token
    // used to start: the new key is fetched once the one-second floor passes.
    *server.keys.write().unwrap() = next.jwks();
    let rotated = next.mint(&next.id_claims("person", "client")).unwrap();
    assert!(
        auth.verify(&rotated).await.is_err(),
        "inside the floor of the last fetch: refused without one"
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 3);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    auth.verify(&rotated).await.unwrap();
    assert_eq!(server.requests.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn jwks_outage_is_5xx_without_cache_and_uses_bounded_stale_keys() {
    let server = JwksServer::start().await;
    let issuer = TestTokenIssuer::es256(&server.url, "first").unwrap();
    *server.keys.write().unwrap() = issuer.jwks();
    let mut config = issuer.config(vec!["client".into()]);
    config.cache_ttl = Duration::from_millis(30);
    config.min_refresh_interval = Duration::from_millis(10);
    config.max_stale = Duration::from_secs(2);
    let cached = JwtAuthenticator::new(config.clone()).unwrap();
    let token = issuer.mint(&issuer.id_claims("person", "client")).unwrap();
    cached.verify(&token).await.unwrap();
    server.available.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(40)).await;
    cached.verify(&token).await.unwrap();
    config.min_refresh_interval = Duration::from_secs(2);
    let uncached = JwtAuthenticator::new(config).unwrap();
    let service = service(vec![Arc::new(uncached)]);
    let bearer = format!("Bearer {token}");
    let (status, headers, _) = call(&service, "/protected", &[("authorization", &bearer)]).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!headers.contains_key("www-authenticate"));
    let requests = server.requests.load(Ordering::SeqCst);
    for _ in 0..3 {
        assert_eq!(
            call(&service, "/protected", &[("authorization", &bearer)])
                .await
                .0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
    assert_eq!(
        server.requests.load(Ordering::SeqCst),
        requests,
        "failed fetches are rate limited"
    );
    tokio::time::sleep(Duration::from_millis(2100)).await;
    assert!(
        cached.verify(&token).await.is_err(),
        "stale lifetime is bounded"
    );
}

struct LegacyProbe;
impl Operation for LegacyProbe {
    const INFO: &'static crucible_server_runtime_rust::OperationInfo = Protected::INFO;
    type Input = crucible_server_runtime_rust::operation::Unit;
    type Output = auth_weather::ProtectedOutput;
}
static LEGACY_SERVICE: ServiceInfo = generated::service_info(
    "example.auth#Legacy",
    "1",
    None,
    &[Protected::INFO],
    &[],
    |_| None,
    None,
    None,
    None,
);

#[test]
fn legacy_authentication_metadata_fails_closed_at_startup() {
    let operations = Operations::builder(&LEGACY_SERVICE)
        .add::<LegacyProbe, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .build()
        .unwrap();
    let error = Crucible::builder(operations)
        .defaults()
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .operation_middleware(AuthenticatorChain::new(vec![id_auth()]))
        .build()
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains(
            "example.auth#Protected: authentication metadata missing; regenerate bindings"
        )
    );
}

#[tokio::test]
async fn scoped_chain_allows_separate_access_token_policy() {
    let service = Crucible::builder(operations())
        .defaults()
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .operation_middleware(AuthenticatorChain::new(vec![access_auth()]).only(&["BearerOnly"]))
        .operation_middleware(AuthenticatorChain::new(vec![id_auth()]).except(&["BearerOnly"]))
        .build()
        .unwrap();
    let id = format!(
        "Bearer {}",
        issuer()
            .mint(&issuer().id_claims("person", "client"))
            .unwrap()
    );
    let access = format!(
        "Bearer {}",
        issuer()
            .mint(&issuer().access_claims("person", "client"))
            .unwrap()
    );
    assert_eq!(
        call(&service, "/bearer", &[("authorization", &id)]).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&service, "/bearer", &[("authorization", &access)])
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&service, "/protected", &[("authorization", &access)])
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn refusal_factory_preserves_modeled_body_and_typed_marker() {
    let chain = AuthenticatorChain::new(vec![id_auth()]).refusal_factory(|refusal| {
        assert_eq!(refusal, FrameworkError::Unauthenticated);
        assert_eq!(
            Context::current().attribute::<FrameworkError>(),
            Some(refusal)
        );
        auth_weather::AccessDeniedException {
            message: "access denied".into(),
        }
        .into()
    });
    let service = Crucible::builder(operations())
        .defaults()
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .operation_middleware(chain)
        .build()
        .unwrap();
    let missing = call(&service, "/protected", &[]).await;
    let wrong = call(
        &service,
        "/protected",
        &[("authorization", "Bearer garbage")],
    )
    .await;
    assert_eq!(missing.0, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.0, StatusCode::UNAUTHORIZED);
    assert_eq!(missing.2, wrong.2);
    assert_eq!(wrong.2["__type"], "AccessDeniedException");
    assert!(wrong.1.contains_key("www-authenticate"));
}
