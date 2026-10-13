//! Server-side idempotency over HTTP: `CreateCity` carries an idempotency
//! token, and `Idempotency` claims it in a store before the operation runs,
//! replays the first success to a retry, refuses a reused token with other
//! input or while the first request runs, and releases the claim when the
//! operation fails before `idempotency::commit`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crucible_server_runtime_rust::idempotency::{
    self, Claim, Fingerprint, Idempotency, IdempotencyKey, IdempotencyRefusal, IdempotencyStore,
    MemoryIdempotencyStore, StoredResponse,
};
use crucible_server_runtime_rust::{Context, Crucible, Error};
use serde_json::{Value, json};
use tokio::sync::Notify;
use weather_service::weather::{CitySummary, ConflictError};
use weather_service::{CityStore, MemoryCities, NewCity, StoredPhoto};

/// What the next `create` does, beyond counting itself.
enum Step {
    /// Fails before the point of no return: a retry runs again.
    FailBeforeCommit,
    /// Commits, then fails: a retry must not run again.
    FailAfterCommit,
    /// Says it entered, then waits to be let go.
    Wait {
        entered: Arc<Notify>,
        release: Arc<Notify>,
    },
}

/// The cities, counting the creates that reach the store: each is one run of
/// the operation. The token is not passed on, so the store's own receipts do
/// not hide a second run; the framework's claim is what is tested.
#[derive(Default)]
struct Counted {
    inner: MemoryCities,
    creates: AtomicUsize,
    script: Mutex<VecDeque<Step>>,
}

impl Counted {
    fn next(&self, step: Step) {
        self.script.lock().unwrap().push_back(step);
    }

    fn creates(&self) -> usize {
        self.creates.load(Ordering::SeqCst)
    }
}

#[crucible_server_runtime_rust::async_trait]
impl CityStore for Counted {
    async fn get(&self, id: &str) -> crucible_server_runtime_rust::Result<Option<CitySummary>> {
        self.inner.get(id).await
    }

    async fn create(
        &self,
        city: NewCity,
        _token: Option<&str>,
    ) -> crucible_server_runtime_rust::Result<CitySummary> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        let step = self.script.lock().unwrap().pop_front();
        match step {
            Some(Step::FailBeforeCommit) => return Err(Error::internal("the database is down")),
            Some(Step::FailAfterCommit) => {
                idempotency::commit().await?;
                return Err(Error::internal("recorded nothing after the side effect"));
            }
            Some(Step::Wait { entered, release }) => {
                entered.notify_one();
                release.notified().await;
            }
            None => {}
        }
        // Each run is a new city, even for the same name.
        let name = format!("{}-{}", city.name, self.creates());
        self.inner.create(NewCity { name, ..city }, None).await
    }

    async fn list(
        &self,
        after: Option<&str>,
        limit: usize,
        names: &[String],
    ) -> crucible_server_runtime_rust::Result<Vec<CitySummary>> {
        self.inner.list(after, limit, names).await
    }

    async fn put_photo(
        &self,
        id: &str,
        photo: StoredPhoto,
    ) -> crucible_server_runtime_rust::Result<()> {
        self.inner.put_photo(id, photo).await
    }

    async fn photo(&self, id: &str) -> crucible_server_runtime_rust::Result<Option<StoredPhoto>> {
        self.inner.photo(id).await
    }
}

/// Who is asking. A real service names its authenticated principal.
fn caller(context: &Context) -> Option<String> {
    context
        .headers()
        .get("x-caller")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

struct Harness {
    endpoint: String,
    service: Crucible,
    cities: Arc<Counted>,
    server: tokio::task::JoinHandle<()>,
    http: reqwest::Client,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
    }
}

struct Answer {
    status: u16,
    request_id: String,
    content_type: String,
    body: Vec<u8>,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

async fn serve(idempotency: Idempotency) -> Harness {
    let cities = Arc::new(Counted::default());
    let service = weather_service::builder(cities.clone())
        .interceptor(idempotency)
        .build()
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn({
        let service = service.clone();
        async move {
            let _ = crucible_server_runtime_rust::host::http_on(service, listener).await;
        }
    });
    // The client runtime installs the process TLS provider raw reqwest uses.
    drop(crucible_client_runtime_rust::Config::new(&endpoint));
    Harness {
        endpoint,
        service,
        cities,
        server,
        http: reqwest::Client::new(),
    }
}

impl Harness {
    async fn create(&self, caller: Option<&str>, body: Value) -> Answer {
        let mut request = self
            .http
            .post(format!("{}/cities", self.endpoint))
            .header("x-expected-revision", "7")
            .json(&body);
        if let Some(caller) = caller {
            request = request.header("x-caller", caller);
        }
        let response = request.send().await.unwrap();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .map(|value| value.to_str().unwrap().to_owned())
                .unwrap_or_default()
        };
        let request_id = header("x-request-id");
        let content_type = header("content-type");
        let status = response.status().as_u16();
        let body = response.bytes().await.unwrap().to_vec();
        Answer {
            status,
            request_id,
            content_type,
            body,
        }
    }
}

fn store() -> Arc<MemoryIdempotencyStore> {
    Arc::new(MemoryIdempotencyStore::new())
}

fn oslo(token: &str) -> Value {
    json!({ "name": "Oslo", "clientToken": token })
}

#[tokio::test]
async fn a_retry_replays_the_first_success_without_running_again() {
    let harness = serve(Idempotency::new(store(), caller)).await;
    let first = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(first.status, 201, "{}", first.json());
    assert_eq!(first.json()["city"]["name"], "Oslo-1");
    let retry = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(retry.status, first.status);
    assert_eq!(retry.body, first.body, "the same bytes");
    assert_eq!(retry.content_type, first.content_type);
    // The retry is its own request, with its own id.
    assert!(!retry.request_id.is_empty());
    assert_ne!(retry.request_id, first.request_id);
    assert_eq!(harness.cities.creates(), 1);

    // Members in another order are the same request.
    let reordered = harness
        .create(
            Some("alice"),
            json!({ "clientToken": "t-1", "name": "Oslo" }),
        )
        .await;
    assert_eq!(reordered.body, first.body);
    assert_eq!(harness.cities.creates(), 1);
}

#[tokio::test]
async fn a_direct_invoke_replays_what_http_created() {
    // An invocation carries no caller header, so both are the anonymous caller.
    let harness = serve(Idempotency::new(store(), caller)).await;
    let first = harness.create(None, oslo("t-1")).await;
    assert_eq!(first.status, 201);
    let invoked = harness
        .service
        .invoke(
            "CreateCity",
            json!({ "name": "Oslo", "clientToken": "t-1", "expectedRevision": 7 }),
        )
        .await;
    assert_eq!(invoked["output"], first.json(), "{invoked}");
    assert_eq!(harness.cities.creates(), 1);
}

#[tokio::test]
async fn the_same_token_with_other_input_is_a_conflict() {
    let harness = serve(Idempotency::new(store(), caller)).await;
    assert_eq!(harness.create(Some("alice"), oslo("t-1")).await.status, 201);
    let other = harness
        .create(
            Some("alice"),
            json!({ "name": "Bergen", "clientToken": "t-1" }),
        )
        .await;
    assert_eq!(other.status, 409);
    let body = other.json();
    assert_eq!(body["__type"], "ConflictException", "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("different request"),
        "{body}"
    );
    assert_eq!(harness.cities.creates(), 1);
}

#[tokio::test]
async fn a_request_without_a_token_is_refused_naming_the_member() {
    let harness = serve(Idempotency::new(store(), caller)).await;
    for body in [json!({ "name": "Oslo" }), oslo("")] {
        let refused = harness.create(Some("alice"), body).await;
        assert_eq!(refused.status, 400);
        let body = refused.json();
        assert_eq!(body["__type"], "ValidationError", "{body}");
        assert_eq!(body["reason"], "fieldValidationFailed", "{body}");
        assert_eq!(body["fieldList"][0]["path"], "clientToken", "{body}");
    }
    assert_eq!(harness.cities.creates(), 0);

    // A service that opts out lets it run, unprotected.
    let optional = serve(Idempotency::new(store(), caller).optional_tokens()).await;
    let created = optional
        .create(Some("alice"), json!({ "name": "Oslo" }))
        .await;
    assert_eq!(created.status, 201);
}

#[tokio::test]
async fn of_two_concurrent_first_requests_one_runs_and_the_other_is_in_flight() {
    let harness = serve(Idempotency::new(store(), caller)).await;
    let (entered, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    harness.cities.next(Step::Wait {
        entered: entered.clone(),
        release: release.clone(),
    });
    let harness = Arc::new(harness);
    let first = tokio::spawn({
        let harness = harness.clone();
        async move { harness.create(Some("alice"), oslo("t-1")).await }
    });
    entered.notified().await;
    let second = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(second.status, 409);
    let body = second.json();
    assert_eq!(body["__type"], "ConflictException", "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("in progress"),
        "{body}"
    );
    release.notify_one();
    let first = first.await.unwrap();
    assert_eq!(first.status, 201);
    let retry = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(retry.body, first.body);
    assert_eq!(harness.cities.creates(), 1);
}

#[tokio::test]
async fn a_failure_before_commit_lets_the_retry_run() {
    let harness = serve(Idempotency::new(store(), caller)).await;
    harness.cities.next(Step::FailBeforeCommit);
    assert_eq!(harness.create(Some("alice"), oslo("t-1")).await.status, 500);
    let retry = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(retry.status, 201);
    assert_eq!(harness.cities.creates(), 2);
}

#[tokio::test]
async fn a_failure_after_commit_keeps_the_claim_so_the_retry_does_not_run() {
    let harness = serve(Idempotency::new(store(), caller)).await;
    harness.cities.next(Step::FailAfterCommit);
    assert_eq!(harness.create(Some("alice"), oslo("t-1")).await.status, 500);
    let retry = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(retry.status, 409);
    assert!(
        retry.json()["message"]
            .as_str()
            .unwrap()
            .contains("in progress")
    );
    assert_eq!(harness.cities.creates(), 1);
}

#[tokio::test]
async fn another_caller_with_the_same_token_is_independent() {
    let harness = serve(Idempotency::new(store(), caller)).await;
    let alice = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(alice.status, 201);
    // Bob's token is his own: neither a replay of Alice's nor a conflict.
    let bob = harness
        .create(
            Some("bob"),
            json!({ "name": "Bergen", "clientToken": "t-1" }),
        )
        .await;
    assert_eq!(bob.status, 201, "{}", bob.json());
    assert_eq!(bob.json()["city"]["name"], "Bergen-2");
    assert_eq!(harness.cities.creates(), 2);
}

#[tokio::test]
async fn a_refusal_factory_answers_with_the_services_own_errors() {
    let idempotency = Idempotency::new(store(), caller).refusal_factory(|refusal| match refusal {
        IdempotencyRefusal::Mismatch | IdempotencyRefusal::InFlight => ConflictError {
            message: format!("client token: {}", refusal.message()),
        }
        .into(),
        other => other.default_error(),
    });
    let harness = serve(idempotency).await;
    assert_eq!(harness.create(Some("alice"), oslo("t-1")).await.status, 201);
    let other = harness
        .create(
            Some("alice"),
            json!({ "name": "Bergen", "clientToken": "t-1" }),
        )
        .await;
    assert_eq!(other.status, 409);
    let body = other.json();
    assert_eq!(body["__type"], "ConflictError", "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .starts_with("client token:"),
        "{body}"
    );
    // The cases it does not answer keep the default.
    let missing = harness
        .create(Some("alice"), json!({ "name": "Oslo" }))
        .await;
    assert_eq!(missing.status, 400);
}

#[tokio::test]
async fn a_store_outage_is_an_internal_error_and_never_runs_the_operation() {
    let store = store();
    let harness = serve(Idempotency::new(store.clone(), caller)).await;
    store.set_unavailable(true);
    let refused = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(refused.status, 500);
    assert!(!String::from_utf8_lossy(&refused.body).contains("unavailable"));
    assert_eq!(harness.cities.creates(), 0);
}

/// A store whose completion write fails, as a database might after the
/// operation succeeded.
struct LosesCompletions(MemoryIdempotencyStore, AtomicBool);

#[crucible_server_runtime_rust::async_trait]
impl IdempotencyStore for LosesCompletions {
    async fn claim(
        &self,
        key: &IdempotencyKey,
        fingerprint: &Fingerprint,
        attempt: &str,
        now: SystemTime,
        expires: SystemTime,
    ) -> crucible_server_runtime_rust::Result<Claim> {
        self.0.claim(key, fingerprint, attempt, now, expires).await
    }

    async fn hold(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        expires: SystemTime,
    ) -> crucible_server_runtime_rust::Result<()> {
        self.0.hold(key, attempt, expires).await
    }

    async fn complete(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: &StoredResponse,
        _: SystemTime,
    ) -> crucible_server_runtime_rust::Result<()> {
        self.1.store(true, Ordering::SeqCst);
        Err(Error::internal("the write timed out"))
    }

    async fn release(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
    ) -> crucible_server_runtime_rust::Result<()> {
        self.0.release(key, attempt).await
    }
}

#[tokio::test]
async fn a_lost_completion_keeps_the_success_and_leaves_the_claim_in_flight() {
    let store = Arc::new(LosesCompletions(
        MemoryIdempotencyStore::new(),
        AtomicBool::new(false),
    ));
    let harness = serve(Idempotency::new(store.clone(), caller)).await;
    let first = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(first.status, 201, "the success stands");
    assert!(store.1.load(Ordering::SeqCst));
    // Until the claim expires, a retry is refused rather than run again.
    let retry = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(retry.status, 409);
    assert_eq!(harness.cities.creates(), 1);
}
