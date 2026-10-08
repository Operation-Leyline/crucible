use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use crucible_auth::{
    JwtAuthenticator, McpAuthenticator, Principal, ScopeEnforcer, test_support::TestTokenIssuer,
};
use crucible_server_runtime_rust::{Context, Crucible, mcp::McpConfig};
use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/scopes.rs"));

const RESOURCE: &str = "https://api.example.com/mcp";
const SCOPE: &str = "weather/mcp";

fn operations() -> crucible_server_runtime_rust::Operations {
    use scoped_weather::{Inherits, OptionalWithScopes, OptsOut, Overrides, Public};
    use std::sync::Arc;
    crucible_server_runtime_rust::Operations::builder(&scoped_weather::SERVICE)
        .add::<Inherits, _, _>(Arc::new(()), |_, _| {
            Box::pin(async {
                assert_eq!(
                    Context::current().attribute::<Principal>().unwrap().subject,
                    "person"
                );
                Ok(())
            })
        })
        .add::<Overrides, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(()) }))
        .add::<OptsOut, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(()) }))
        .add::<Public, _, _>(Arc::new(()), |_, _| {
            Box::pin(async {
                assert!(Context::current().attribute::<Principal>().is_none());
                Ok(())
            })
        })
        .add::<OptionalWithScopes, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(()) }))
        .build()
        .unwrap()
}
fn authorization(
    issuer: &TestTokenIssuer,
    resource: &str,
    compatibility: bool,
) -> McpAuthenticator {
    let verifier = JwtAuthenticator::with_jwks(
        issuer.access_config(vec!["mcp-client".into()]),
        issuer.jwks(),
    )
    .unwrap();
    let auth = McpAuthenticator::new(resource, vec![verifier], SCOPE).unwrap();
    if compatibility {
        auth.allow_unbound_cognito_tokens()
    } else {
        auth
    }
}

fn service(issuer: &TestTokenIssuer, compatibility: bool) -> Crucible {
    Crucible::builder(operations())
        .defaults()
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .mcp(McpConfig::new().authorization(authorization(issuer, RESOURCE, compatibility)))
        .interceptor(ScopeEnforcer::strict())
        .build()
        .unwrap()
}

async fn request(
    service: &Crucible,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, String, Value) {
    let mut request = Request::post(path).header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = service
        .handle(request.body(Body::from(body.to_string())).unwrap())
        .await;
    let status = response.status();
    let challenge = response
        .headers()
        .get("www-authenticate")
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, challenge, body)
}

fn message(method: &str) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":method,"params":{}})
}

#[tokio::test]
async fn first_request_authentication_covers_every_message_and_scope_step_up() {
    let issuer = TestTokenIssuer::es256("https://issuer.example", "key").unwrap();
    let service = service(&issuer, false);
    for body in [
        message("initialize"),
        message("tools/list"),
        message("ping"),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!("malformed"),
    ] {
        let (status, challenge, _) = request(&service, "/mcp", None, body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            challenge,
            "Bearer scope=\"weather/mcp\", resource_metadata=\"https://api.example.com/.well-known/oauth-protected-resource/mcp\""
        );
    }
    let mut claims = issuer.access_claims("person", "mcp-client");
    claims["aud"] = json!(RESOURCE);
    claims["scope"] = json!(SCOPE);
    let token = issuer.mint(&claims).unwrap();
    assert_eq!(
        request(&service, "/mcp", Some(&token), message("tools/list"))
            .await
            .0,
        StatusCode::OK
    );
    let call = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"Inherits","arguments":{}}});
    let (status, challenge, _) = request(&service, "/mcp", Some(&token), call.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(challenge.contains("scope=\"weather/read\""));
    assert!(challenge.contains("error=\"insufficient_scope\""));
    assert!(challenge.contains(
        "resource_metadata=\"https://api.example.com/.well-known/oauth-protected-resource/mcp\""
    ));
    claims["scope"] = json!("weather/mcp weather/read");
    assert_eq!(
        request(&service, "/mcp", Some(&issuer.mint(&claims).unwrap()), call)
            .await
            .0,
        StatusCode::OK
    );
    let ordinary = service
        .handle(
            Request::get("/public")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(ordinary.status(), StatusCode::OK);
    assert!(!ordinary.headers().contains_key("www-authenticate"));
}

#[tokio::test]
async fn access_token_boundary_and_explicit_cognito_compatibility() {
    let issuer = TestTokenIssuer::es256("https://issuer.example", "key").unwrap();
    for compatibility in [false, true] {
        let service = service(&issuer, compatibility);
        let mut good = issuer.access_claims("person", "mcp-client");
        good["aud"] = json!(RESOURCE);
        good["scope"] = json!(SCOPE);
        for (name, value) in [
            ("aud", json!("https://other.example/mcp")),
            ("aud", json!([RESOURCE])),
            ("client_id", json!("other-client")),
            ("token_use", json!("id")),
            ("exp", json!(0)),
            ("iss", json!("https://other-issuer.example")),
        ] {
            let mut bad = good.clone();
            bad[name] = value;
            assert_eq!(
                request(
                    &service,
                    "/mcp",
                    Some(&issuer.mint(&bad).unwrap()),
                    message("ping")
                )
                .await
                .0,
                StatusCode::UNAUTHORIZED,
                "{name}, compatibility={compatibility}"
            );
        }
        let mut unbound = good.clone();
        unbound.as_object_mut().unwrap().remove("aud");
        assert_eq!(
            request(
                &service,
                "/mcp",
                Some(&issuer.mint(&unbound).unwrap()),
                message("ping")
            )
            .await
            .0,
            if compatibility {
                StatusCode::OK
            } else {
                StatusCode::UNAUTHORIZED
            }
        );
        good["scope"] = json!("openid weather/read");
        let (status, challenge, _) = request(
            &service,
            "/mcp",
            Some(&issuer.mint(&good).unwrap()),
            message("ping"),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(challenge.contains("scope=\"weather/mcp\""));
        assert_eq!(
            request(&service, "/mcp?access_token=ignored", None, message("ping"))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        let response = service
            .handle(
                Request::post("/mcp")
                    .header("authorization", "Bearer one")
                    .header("authorization", "Bearer two")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn discovery_custom_path_and_multiple_issuers_over_real_http() {
    let first = TestTokenIssuer::es256("https://z-issuer.example", "key1").unwrap();
    let second = TestTokenIssuer::es256("https://a-issuer.example", "key2").unwrap();
    let verifiers = [&first, &second, &first]
        .into_iter()
        .map(|issuer| {
            JwtAuthenticator::with_jwks(
                issuer.access_config(vec!["mcp-client".into()]),
                issuer.jwks(),
            )
            .unwrap()
        })
        .collect();
    let resource = "https://api.example.com/agents/mcp";
    let service = Crucible::builder(operations())
        .defaults()
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .mcp(
            McpConfig::new()
                .path("/agents/mcp")
                .authorization(McpAuthenticator::new(resource, verifiers, SCOPE).unwrap()),
        )
        .build()
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, service.into_router()).await.unwrap();
    });
    drop(crucible_client_runtime_rust::Config::new(&endpoint));
    let http = reqwest::Client::new();
    let url = format!("{endpoint}/.well-known/oauth-protected-resource/agents/mcp");
    let response = http
        .get(&url)
        .header("host", "attacker.example")
        .header("origin", "https://client.example")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({
            "resource":resource,"authorization_servers":["https://a-issuer.example","https://z-issuer.example"],
            "scopes_supported":["weather/admin","weather/mcp","weather/read","weather/write"],
            "bearer_methods_supported":["header"]
        })
    );
    assert_eq!(
        http.head(&url)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .len(),
        0
    );
    assert_eq!(http.post(&url).send().await.unwrap().status().as_u16(), 405);
    let response = http
        .post(format!("{endpoint}/agents/mcp"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 401);
    assert!(
        response.headers()["www-authenticate"]
            .to_str()
            .unwrap()
            .contains("https://api.example.com/.well-known/oauth-protected-resource/agents/mcp")
    );
    let mut claims = second.access_claims("person", "mcp-client");
    claims["aud"] = json!(resource);
    claims["scope"] = json!(SCOPE);
    assert_eq!(
        http.post(format!("{endpoint}/agents/mcp"))
            .bearer_auth(second.mint(&claims).unwrap())
            .header("content-type", "application/json")
            .json(&message("ping"))
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        200
    );
    server.abort();
}

#[test]
fn configuration_fails_closed() {
    let issuer = TestTokenIssuer::es256("https://issuer.example", "key").unwrap();
    let id =
        JwtAuthenticator::with_jwks(issuer.config(vec!["client".into()]), issuer.jwks()).unwrap();
    assert!(McpAuthenticator::new(RESOURCE, vec![id], SCOPE).is_err());
    for scope in [
        "",
        "openid",
        "weather/",
        "/mcp",
        "weather/mcp other",
        "weather/\"bad",
    ] {
        let access =
            JwtAuthenticator::with_jwks(issuer.access_config(vec!["client".into()]), issuer.jwks())
                .unwrap();
        assert!(McpAuthenticator::new(RESOURCE, vec![access], scope).is_err());
    }
    for resource in [
        "http://api.example.com/mcp",
        "https://user@api.example.com/mcp",
        "https://api.example.com/wrong",
        "https://api.example.com/mcp?query",
        "https://api.example.com/mcp#fragment",
    ] {
        assert!(
            Crucible::builder(operations())
                .defaults()
                .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
                .mcp(McpConfig::new().authorization(authorization(&issuer, resource, false)))
                .build()
                .is_err()
        );
    }
    assert!(
        Crucible::builder(operations())
            .defaults()
            .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
            .mcp(
                McpConfig::new()
                    .authorization(authorization(&issuer, RESOURCE, false))
                    .endpoint_guard(|_: &axum::http::request::Parts| {
                        crucible_server_runtime_rust::mcp::EndpointAdmission::Admit
                    })
            )
            .build()
            .is_err()
    );
}

#[tokio::test]
async fn verifier_outage_is_not_an_invalid_token_and_discovery_remains_available() {
    let issuer = TestTokenIssuer::es256("https://issuer.example", "key").unwrap();
    let mut config = issuer.access_config(vec!["mcp-client".into()]);
    // An owned, closed listener gives a deterministic connection refusal, not a public outage.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    config.jwks_url = format!("https://{}/jwks", listener.local_addr().unwrap());
    drop(listener);
    let auth = McpAuthenticator::new(
        RESOURCE,
        vec![JwtAuthenticator::new(config).unwrap()],
        SCOPE,
    )
    .unwrap();
    let service = Crucible::builder(operations())
        .defaults()
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .mcp(McpConfig::new().authorization(auth))
        .build()
        .unwrap();
    let mut claims = issuer.access_claims("person", "mcp-client");
    claims["aud"] = json!(RESOURCE);
    claims["scope"] = json!(SCOPE);
    let (status, challenge, _) = request(
        &service,
        "/mcp",
        Some(&issuer.mint(&claims).unwrap()),
        message("ping"),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(challenge.is_empty());
    let response = service
        .handle(
            Request::get("/.well-known/oauth-protected-resource/mcp")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn authorization_is_opt_in_and_root_metadata_is_not_registered() {
    let ordinary = Crucible::builder(operations())
        .defaults()
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .mcp(McpConfig::new())
        .build()
        .unwrap();
    assert_eq!(
        request(&ordinary, "/mcp", None, message("ping")).await.0,
        StatusCode::OK
    );
    let issuer = TestTokenIssuer::es256("https://issuer.example", "key").unwrap();
    let protected = service(&issuer, false);
    for (service, path) in [
        (&ordinary, "/.well-known/oauth-protected-resource/mcp"),
        (&protected, "/.well-known/oauth-protected-resource"),
    ] {
        let response = service
            .handle(Request::get(path).body(Body::empty()).unwrap())
            .await;
        assert!(!response.headers().contains_key("www-authenticate"));
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert!(body.get("resource").is_none());
    }
}
