//! `ApiKeyAuthenticator` beside `JwtAuthenticator` in one chain, over real HTTP:
//! keys and tokens each reach the authenticator that owns them, and the key's
//! tenant reaches the handler.
use axum::http::StatusCode;
use crucible_auth::{
    ApiKeyAuthenticator, ApiKeyRecord, ApiKeyState, ApiKeyStore, Authenticator, AuthenticatorChain,
    JwtAuthenticator, Principal, api_key_digest,
    test_support::{InMemoryApiKeyStore, TestTokenIssuer},
};
use crucible_server_runtime_rust::{
    Context, Crucible, ExceptionTranslator, FrameworkError, OperationMiddlewareExt, Operations,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

include!(concat!(env!("OUT_DIR"), "/auth_chain.rs"));
use auth_weather::{BearerOnly, Identity, KeyOnly, Optional, Protected, Public};

const KEY: &str = "pgn_e2e_0123456789";
const REVOKED: &str = "pgn_e2e_revoked";
const DOWN: &str = "pgn_e2e_store_down";

type Tenants = Arc<Mutex<Vec<Option<String>>>>;

fn identity() -> Identity {
    let principal = Context::current().attribute::<Principal>();
    Identity {
        subject: principal.as_ref().map(|p| p.subject.clone()),
        kind: principal.map(|p| format!("{:?}", p.token_kind)),
    }
}

/// `BearerOnly` records the tenant its handler sees; the other operations are
/// outside the chain under test.
fn operations(tenants: Tenants) -> Operations {
    Operations::builder(&auth_weather::SERVICE)
        .add::<BearerOnly, _, _>(tenants, |tenants, _| async move {
            let principal = Context::current().attribute::<Principal>();
            tenants
                .lock()
                .unwrap()
                .push(principal.and_then(|p| p.tenant));
            Ok(identity())
        })
        .add::<Protected, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .add::<Optional, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .add::<Public, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .add::<KeyOnly, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .build()
        .unwrap()
}

async fn serve(service: Crucible) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/bearer", listener.local_addr().unwrap());
    let router = service.into_router();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (url, task)
}

#[tokio::test]
async fn api_keys_and_jwts_share_a_chain_and_the_tenant_reaches_the_handler() {
    let issuer = TestTokenIssuer::new("https://issuer.example", "rsa-1").unwrap();
    let token = {
        let mut claims = issuer.id_claims("person", "client");
        // A claim never sets the tenant: only a credential store does.
        claims["tenant"] = json!("claimed-workspace");
        issuer.mint(&claims).unwrap()
    };
    let store = InMemoryApiKeyStore::new();
    store.insert(
        KEY,
        ApiKeyRecord::new("key-7", "workspace-9", ApiKeyState::Active)
            .with_attributes(json!({"name": "ci"})),
    );
    store.insert(
        REVOKED,
        ApiKeyRecord::new("key-8", "workspace-9", ApiKeyState::Revoked),
    );
    store.insert(
        DOWN,
        ApiKeyRecord::new("key-9", "workspace-9", ApiKeyState::Active),
    );
    for keys_first in [true, false] {
        let api_keys: Arc<dyn Authenticator> =
            Arc::new(ApiKeyAuthenticator::new("pgn_", store.clone()).unwrap());
        let jwt: Arc<dyn Authenticator> = Arc::new(
            JwtAuthenticator::with_jwks(issuer.config(vec!["client".into()]), issuer.jwks())
                .unwrap(),
        );
        let authenticators = if keys_first {
            vec![api_keys, jwt]
        } else {
            vec![jwt, api_keys]
        };
        let tenants = Tenants::default();
        let service = Crucible::builder(operations(Arc::clone(&tenants)))
            .defaults()
            .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
            .operation_middleware(AuthenticatorChain::new(authenticators).only(&["BearerOnly"]))
            .build()
            .unwrap();
        let (url, task) = serve(service).await;
        // After JwtAuthenticator::new, which installs the rustls provider reqwest needs.
        let client = reqwest::Client::new();
        let call = |credential: &str| client.get(&url).bearer_auth(credential).send();

        let response = call(KEY).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "keys_first={keys_first}");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["subject"], "key-7");
        assert_eq!(body["kind"], "ApiKey");

        let reads = store.lookups();
        let response = call(&token).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["subject"], "person");
        assert_eq!(body["kind"], "Id");
        assert_eq!(store.lookups(), reads, "a JWT never costs a key lookup");
        assert_eq!(
            *tenants.lock().unwrap(),
            [Some("workspace-9".to_owned()), None]
        );

        for credential in ["pgn_e2e_forged", REVOKED, "garbage"] {
            let response = call(credential).await.unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{credential}");
            assert_eq!(
                response.headers()["www-authenticate"],
                "Bearer error=\"invalid_token\""
            );
        }
        let response = client.get(&url).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // An outage is a sanitized server failure, never a refusal.
        store.set_unavailable(true);
        let response = call(DOWN).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!response.headers().contains_key("www-authenticate"));
        let text = response.text().await.unwrap();
        assert!(!text.contains(DOWN) && !text.contains(&api_key_digest(DOWN)));
        // The valid key is still served from cache meanwhile.
        assert_eq!(call(KEY).await.unwrap().status(), StatusCode::OK);
        store.set_unavailable(false);
        task.abort();
    }
}

/// The service's own store client error, which it translates for its
/// operations. The key table happens to use the same client.
#[derive(Debug)]
struct StoreDown;

impl std::fmt::Display for StoreDown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("store client failed")
    }
}

impl std::error::Error for StoreDown {}

/// Fails with `StoreDown` for one digest, like a table partition that is down.
struct PartlyDown {
    inner: InMemoryApiKeyStore,
    down: String,
}

#[crucible_server_runtime_rust::async_trait]
impl ApiKeyStore for PartlyDown {
    async fn lookup(
        &self,
        digest: &str,
    ) -> Result<Option<ApiKeyRecord>, crucible_server_runtime_rust::Error> {
        if digest == self.down {
            return Err(StoreDown.into());
        }
        self.inner.lookup(digest).await
    }
}

#[tokio::test]
async fn a_rule_for_the_services_store_errors_never_translates_an_authentication_outage() {
    let store = InMemoryApiKeyStore::new();
    for key in [KEY, DOWN] {
        store.insert(
            key,
            ApiKeyRecord::new("key-7", "workspace-9", ApiKeyState::Active),
        );
    }
    let keys = ApiKeyAuthenticator::new(
        "pgn_",
        PartlyDown {
            inner: store,
            down: api_key_digest(DOWN),
        },
    )
    .unwrap();
    let operations = Operations::builder(&auth_weather::SERVICE)
        .add::<BearerOnly, _, _>(Arc::new(()), |_, _| async {
            Err(crucible_server_runtime_rust::Error::new(StoreDown))
        })
        .add::<Protected, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .add::<Optional, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .add::<Public, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .add::<KeyOnly, _, _>(Arc::new(()), |_, _| async { Ok(identity()) })
        .build()
        .unwrap();
    let service = Crucible::builder(operations)
        .defaults()
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .operation_middleware(
            AuthenticatorChain::new(vec![Arc::new(keys) as Arc<dyn Authenticator>])
                .only(&["BearerOnly"]),
        )
        // A rule meant for the operations' own store failures.
        .exception_translator(
            ExceptionTranslator::new().translate(|_: &StoreDown| FrameworkError::Forbidden.into()),
        )
        .build()
        .unwrap();
    let (url, task) = serve(service).await;
    // Constructing a JwtAuthenticator installs the rustls provider reqwest needs.
    let issuer = TestTokenIssuer::es256("https://issuer.example", "key").unwrap();
    JwtAuthenticator::with_jwks(issuer.config(vec!["client".into()]), issuer.jwks()).unwrap();
    let client = reqwest::Client::new();
    // The operation's failure is the service's to translate...
    let response = client.get(&url).bearer_auth(KEY).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // ...but the same error behind the key store's outage stays a server failure.
    let response = client.get(&url).bearer_auth(DOWN).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    task.abort();
}
