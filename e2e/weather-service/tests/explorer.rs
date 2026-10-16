use std::net::SocketAddr;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, request::Parts};
use axum::response::IntoResponse;
use crucible_auth::{
    AuthenticateResult, Authenticator, AuthenticatorChain, EndpointAuthenticator, Principal,
    TokenKind,
};
use crucible_server_runtime_rust::endpoint::EndpointAdmission;
use crucible_server_runtime_rust::explorer::Explorer;
use crucible_server_runtime_rust::{
    Crucible, Hook, Job, Middleware, Operations, operation::AuthScheme,
};
use serde_json::{Value, json};
use weather_service::{MemoryCities, weather};

fn service(explorer: Explorer) -> Crucible {
    weather_service::builder(Arc::new(MemoryCities::default()))
        .explorer(explorer)
        .build()
        .unwrap()
}
fn request(path: &str, peer: Option<&str>) -> Request<Body> {
    let mut request = Request::get(path).body(Body::empty()).unwrap();
    if let Some(peer) = peer {
        request
            .extensions_mut()
            .insert(peer.parse::<SocketAddr>().unwrap());
    }
    request
}
async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

#[tokio::test]
async fn every_asset_has_security_headers_and_code_is_revalidated() {
    let service = service(Explorer::new(weather::DESCRIPTION));
    for (name, mime, revalidate) in [
        ("", "text/html; charset=utf-8", false),
        ("index.html", "text/html; charset=utf-8", false),
        ("explorer.js", "text/javascript; charset=utf-8", true),
        ("explorer.css", "text/css; charset=utf-8", true),
        ("try-it.js", "text/javascript; charset=utf-8", true),
        ("try-it.css", "text/css; charset=utf-8", true),
        ("describe.json", "application/json", false),
        ("describe.js", "text/javascript; charset=utf-8", false),
        ("config.js", "text/javascript; charset=utf-8", false),
    ] {
        let response = service
            .handle(request(
                &format!("/explorer/{name}"),
                Some("127.0.0.1:1234"),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::OK, "{name}");
        assert_eq!(response.headers()["content-type"], mime);
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        assert_eq!(
            response.headers()["content-security-policy"],
            "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'"
        );
        assert_eq!(
            response.headers()["cache-control"],
            if revalidate {
                "private, no-cache"
            } else {
                "no-store"
            }
        );
        assert!(
            !to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .is_empty()
        );
    }
    let response = service
        .handle(request("/explorer", Some("127.0.0.1:1")))
        .await;
    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(response.headers()["location"], "/explorer/");
    let mut head = request("/explorer/explorer.js", Some("[::1]:1"));
    *head.method_mut() = axum::http::Method::HEAD;
    let response = service.handle(head).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        !response.headers()["content-length"]
            .to_str()
            .unwrap()
            .is_empty()
    );
    assert!(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn default_admission_denies_non_loopback_and_unknown_peers_on_all_paths() {
    let service = service(Explorer::new(weather::DESCRIPTION));
    for peer in [None, Some("192.0.2.1:1"), Some("[2001:db8::1]:1")] {
        for path in [
            "/explorer",
            "/explorer/describe.json",
            "/explorer/unknown",
            "/explorer/try-it.js",
        ] {
            assert_eq!(
                service.handle(request(path, peer)).await.status(),
                StatusCode::FORBIDDEN
            );
        }
    }
    let mut forged = request("/explorer/", None);
    forged
        .headers_mut()
        .insert("x-forwarded-for", "127.0.0.1".parse().unwrap());
    assert_eq!(service.handle(forged).await.status(), StatusCode::FORBIDDEN);
    for peer in ["127.0.0.1:1", "[::1]:1", "[::ffff:127.0.0.1]:1"] {
        assert_eq!(
            service
                .handle(request("/explorer/", Some(peer)))
                .await
                .status(),
            StatusCode::OK
        );
    }
    let mut connected = request("/explorer/", None);
    connected
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(
            "127.0.0.1:1".parse::<SocketAddr>().unwrap(),
        ));
    assert_eq!(service.handle(connected).await.status(), StatusCode::OK);
    // Prefix ownership must not catch similarly named ordinary paths.
    assert_ne!(
        service
            .handle(request("/explorer-other", None))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

struct Count(Arc<AtomicUsize>);
impl Middleware for Count {
    fn before<'a>(&'a self, _: &'a mut Job) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Hook::Ready(Ok(()))
    }
}

#[tokio::test]
async fn custom_guard_replaces_loopback_and_runs_before_transport_for_every_resource() {
    let calls = Arc::new(AtomicUsize::new(0));
    let transport = Arc::new(AtomicUsize::new(0));
    let guard_calls = calls.clone();
    let service = weather_service::builder(Arc::new(MemoryCities::default()))
        .middleware(Count(transport.clone()))
        .explorer(
            Explorer::new(weather::DESCRIPTION).endpoint_guard(move |parts: &Parts| {
                guard_calls.fetch_add(1, Ordering::SeqCst);
                if parts
                    .headers
                    .get("x-explorer-key")
                    .is_some_and(|v| v == "secret")
                {
                    EndpointAdmission::Admit
                } else {
                    EndpointAdmission::Respond(StatusCode::UNAUTHORIZED.into_response())
                }
            }),
        )
        .build()
        .unwrap();
    for path in ["/explorer/", "/explorer/describe.json", "/explorer/missing"] {
        assert_eq!(
            service.handle(request(path, None)).await.status(),
            StatusCode::UNAUTHORIZED
        );
        let mut r = request(path, Some("192.0.2.1:1"));
        r.headers_mut()
            .insert("x-explorer-key", "secret".parse().unwrap());
        assert_eq!(
            service.handle(r).await.status(),
            if path.ends_with("missing") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::OK
            }
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    assert_eq!(transport.load(Ordering::SeqCst), 0);
    assert_eq!(
        service.handle(request("/ping", None)).await.status(),
        StatusCode::OK
    );
    assert_eq!(transport.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn explicit_remote_access_serves_on_lambda_with_no_peer() {
    let event = json!({"version":"2.0", "routeKey":"$default", "rawPath":"/explorer/describe.json", "rawQueryString":"", "headers":{},
        "requestContext":{"stage":"dev", "requestId":"explorer", "http":{"method":"GET", "path":"/explorer/describe.json", "protocol":"HTTP/1.1", "sourceIp":"127.0.0.1"}}, "isBase64Encoded":false});
    let local = service(Explorer::new(weather::DESCRIPTION));
    assert_eq!(
        crucible_server_runtime_rust::host::lambda_event(&local, event.clone())
            .await
            .unwrap()["statusCode"],
        403
    );
    let remote = service(Explorer::new(weather::DESCRIPTION).allow_remote());
    let answer = crucible_server_runtime_rust::host::lambda_event(&remote, event)
        .await
        .unwrap();
    assert_eq!(answer["statusCode"], 200, "{answer}");
    assert!(
        answer["body"]
            .as_str()
            .unwrap()
            .contains("crucible.describe/1")
    );
}

#[tokio::test]
async fn native_http_host_supplies_socket_peer() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(crucible_server_runtime_rust::host::http_on(
        service(Explorer::new(weather::DESCRIPTION)),
        listener,
    ));
    drop(crucible_client_runtime_rust::Config::new(&endpoint));
    let response = reqwest::get(format!("{endpoint}/explorer/")).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    server.abort();
}

#[test]
fn invalid_paths_and_modeled_prefix_collisions_fail_prepare() {
    for path in [
        "/cities",
        "/cities/c-abc12345/photo",
        "/ping",
        "/",
        "relative",
        "/explorer?x",
        "/explorer#x",
        "/../explorer",
        "/ex%70lorer",
    ] {
        let result = weather_service::builder(Arc::new(MemoryCities::default()))
            .explorer(Explorer::new(weather::DESCRIPTION).path(path))
            .build();
        assert!(
            matches!(
                result,
                Err(crucible_server_runtime_rust::BuildError::Prepare(_))
            ),
            "{path}"
        );
    }
}

#[tokio::test]
async fn configuration_is_prepared_once_with_title_and_read_only() {
    let service = service(
        Explorer::new(weather::DESCRIPTION)
            .path("/dev/api/")
            .title("Weather <dev>")
            .read_only(true)
            .allow_remote(),
    );
    let response = service.handle(request("/dev/api/config.js", None)).await;
    let config = String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(config.contains("\"readOnly\":true"));
    assert!(config.contains("Weather <dev>"));
    assert!(config.contains("\"prefixSegments\":2"));
}

#[allow(dead_code)]
mod auth_bindings {
    include!(concat!(env!("OUT_DIR"), "/auth_chain.rs"));
}
use auth_bindings::auth_weather as auth;

struct Token;
#[crucible_server_runtime_rust::async_trait]
impl Authenticator for Token {
    fn supports(&self, scheme: &AuthScheme) -> bool {
        matches!(scheme, AuthScheme::Bearer)
    }
    fn has_credentials(&self, parts: &Parts, _: &AuthScheme) -> bool {
        parts.headers.contains_key("authorization")
    }
    async fn authenticate(&self, parts: &Parts, _: &AuthScheme) -> AuthenticateResult {
        match parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
        {
            Some("Bearer token") => AuthenticateResult::Authenticated(Principal::new(
                "developer",
                "local",
                TokenKind::Access,
            )),
            Some("Bearer outage") => AuthenticateResult::Unavailable(
                crucible_server_runtime_rust::Error::internal("private outage"),
            ),
            _ => AuthenticateResult::Refused,
        }
    }
}

#[tokio::test]
async fn endpoint_auth_adapter_and_normal_route_auth_are_independent() {
    // Each operation has an output of its own with the same members.
    fn identity<O: serde::de::DeserializeOwned>() -> O {
        serde_json::from_value(json!({"subject": "developer"})).unwrap()
    }
    let operations = Operations::builder(&auth::SERVICE)
        .add::<auth::Protected, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .add::<auth::Public, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .add::<auth::BearerOnly, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .build()
        .unwrap();
    let service = Crucible::builder(operations)
        .defaults()
        .on_rejection(weather::SERVICE.reject.unwrap())
        .operation_middleware(AuthenticatorChain::new(vec![Arc::new(Token)]))
        .explorer(
            Explorer::new(auth::DESCRIPTION)
                .endpoint_guard(EndpointAuthenticator::new(Token, AuthScheme::Bearer).unwrap()),
        )
        .build()
        .unwrap();
    for path in ["/explorer/", "/explorer/describe.json", "/bearer"] {
        for (token, expected) in [
            (None, 401),
            (Some("Bearer bad"), 401),
            (Some("Bearer token"), 200),
        ] {
            let mut r = request(path, None);
            if let Some(token) = token {
                r.headers_mut()
                    .insert("authorization", token.parse().unwrap());
            }
            let response = service.handle(r).await;
            assert_eq!(response.status().as_u16(), expected, "{path}");
            if expected == 401 {
                assert!(response.headers().contains_key("www-authenticate"));
            }
        }
    }
    let description = {
        let mut r = request("/explorer/describe.json", None);
        r.headers_mut()
            .insert("authorization", "Bearer token".parse().unwrap());
        json_body(service.handle(r).await).await
    };
    assert_eq!(
        description["operations"]["example.auth#BearerOnly"]["uri"],
        "/bearer"
    );
    let mut outage = request("/explorer/describe.json", None);
    outage
        .headers_mut()
        .insert("authorization", "Bearer outage".parse().unwrap());
    let response = service.handle(outage).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .is_empty()
    );
}
