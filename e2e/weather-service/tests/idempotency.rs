//! Server-side idempotency over HTTP: `CreateCity` carries an idempotency
//! token, and `Idempotency` claims it in a store before the operation runs,
//! replays the first success to a retry, refuses a reused token with other
//! input or while the first request runs, and releases the claim when the
//! operation fails before `idempotency::commit`, or after
//! `idempotency::abandon` takes the commit back.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crucible_server_runtime_rust::idempotency::{
    self, Claim, Fingerprint, Idempotency, IdempotencyKey, IdempotencyRefusal, IdempotencyStore,
    MemoryIdempotencyStore, Ownership, StoredResponse,
};
use crucible_server_runtime_rust::mcp::McpConfig;
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
    /// Commits, finds its step refused (nothing happened), abandons the
    /// commit and fails: a retry runs again.
    AbandonThenFail,
    /// Abandons without having committed, then fails: a retry runs again.
    AbandonWithoutCommitThenFail,
    /// Commits, abandons, then succeeds anyway: a retry replays.
    AbandonThenSucceed,
    /// Commits, abandons, commits again, then fails: a retry must not run.
    RecommitThenFail,
    /// Says it entered, then waits to be let go.
    Wait {
        entered: Arc<Notify>,
        release: Arc<Notify>,
    },
    /// Waits like `Wait`, then commits and takes its irreversible step.
    WaitThenSend {
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
    /// Irreversible steps taken: each must happen at most once per token.
    sends: AtomicUsize,
    script: Mutex<VecDeque<Step>>,
}

impl Counted {
    fn next(&self, step: Step) {
        self.script.lock().unwrap().push_back(step);
    }

    fn creates(&self) -> usize {
        self.creates.load(Ordering::SeqCst)
    }

    fn sends(&self) -> usize {
        self.sends.load(Ordering::SeqCst)
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
        let run = self.creates.fetch_add(1, Ordering::SeqCst) + 1;
        let step = self.script.lock().unwrap().pop_front();
        match step {
            Some(Step::FailBeforeCommit) => return Err(Error::internal("the database is down")),
            Some(Step::FailAfterCommit) => {
                idempotency::commit().await?;
                return Err(Error::internal("recorded nothing after the side effect"));
            }
            Some(Step::AbandonThenFail) => {
                idempotency::commit().await?;
                idempotency::abandon()?;
                return Err(Error::internal("the provider refused; nothing was sent"));
            }
            Some(Step::AbandonWithoutCommitThenFail) => {
                idempotency::abandon()?;
                return Err(Error::internal("failed before the point of no return"));
            }
            Some(Step::AbandonThenSucceed) => {
                idempotency::commit().await?;
                idempotency::abandon()?;
            }
            Some(Step::RecommitThenFail) => {
                idempotency::commit().await?;
                idempotency::abandon()?;
                idempotency::commit().await?;
                return Err(Error::internal("recorded nothing after the side effect"));
            }
            Some(Step::Wait { entered, release }) => {
                entered.notify_one();
                release.notified().await;
            }
            Some(Step::WaitThenSend { entered, release }) => {
                entered.notify_one();
                release.notified().await;
                idempotency::commit().await?;
                self.sends.fetch_add(1, Ordering::SeqCst);
            }
            None => {}
        }
        // Each run is a new city, even for the same name.
        let name = format!("{}-{run}", city.name);
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

/// A clock a test moves by hand, for leases that lapse.
#[derive(Clone)]
struct Clock(Arc<Mutex<SystemTime>>);

impl Clock {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(SystemTime::now())))
    }

    fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }

    fn reader(&self) -> impl Fn() -> SystemTime + Send + Sync + 'static {
        let clock = self.0.clone();
        move || *clock.lock().unwrap()
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
async fn a_failure_after_abandoning_the_commit_lets_the_retry_run() {
    let harness = serve(Idempotency::new(store(), caller)).await;
    harness.cities.next(Step::AbandonThenFail);
    assert_eq!(harness.create(Some("alice"), oslo("t-1")).await.status, 500);
    let retry = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(retry.status, 201, "{}", retry.json());
    assert_eq!(retry.json()["city"]["name"], "Oslo-2");
    assert_eq!(harness.cities.creates(), 2);
}

#[tokio::test]
async fn abandoning_without_a_commit_still_lets_the_retry_run() {
    let harness = serve(Idempotency::new(store(), caller)).await;
    harness.cities.next(Step::AbandonWithoutCommitThenFail);
    assert_eq!(harness.create(Some("alice"), oslo("t-1")).await.status, 500);
    let retry = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(retry.status, 201, "{}", retry.json());
    assert_eq!(harness.cities.creates(), 2);
}

#[tokio::test]
async fn a_success_after_abandoning_the_commit_replays() {
    let harness = serve(Idempotency::new(store(), caller)).await;
    harness.cities.next(Step::AbandonThenSucceed);
    let first = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(first.status, 201, "{}", first.json());
    let retry = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(retry.status, 201);
    assert_eq!(retry.body, first.body, "the same bytes");
    assert_eq!(harness.cities.creates(), 1);
}

#[tokio::test]
async fn a_commit_after_abandoning_keeps_the_claim_again() {
    let harness = serve(Idempotency::new(store(), caller)).await;
    harness.cities.next(Step::RecommitThenFail);
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

/// A store that fails its completion writes, or answers replays with a
/// response written before a deploy, as a database might.
#[derive(Default)]
struct Faulty {
    inner: MemoryIdempotencyStore,
    fail_completions: AtomicBool,
    failed: AtomicBool,
    stale: AtomicBool,
}

#[crucible_server_runtime_rust::async_trait]
impl IdempotencyStore for Faulty {
    async fn claim(
        &self,
        key: &IdempotencyKey,
        fingerprint: &Fingerprint,
        attempt: &str,
        now: SystemTime,
        expires: SystemTime,
    ) -> crucible_server_runtime_rust::Result<Claim> {
        let claim = self
            .inner
            .claim(key, fingerprint, attempt, now, expires)
            .await?;
        Ok(match claim {
            Claim::Replay { fingerprint, .. } if self.stale.load(Ordering::SeqCst) => {
                Claim::Replay {
                    fingerprint,
                    response: StoredResponse::new(
                        r#"{"apiVersion":"1999-01-01","output":{"city":{}}}"#,
                    ),
                }
            }
            claim => claim,
        })
    }

    async fn hold(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        expires: SystemTime,
    ) -> crucible_server_runtime_rust::Result<Ownership> {
        self.inner.hold(key, attempt, expires).await
    }

    async fn complete(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        response: &StoredResponse,
        expires: SystemTime,
    ) -> crucible_server_runtime_rust::Result<Ownership> {
        if self.fail_completions.load(Ordering::SeqCst) {
            self.failed.store(true, Ordering::SeqCst);
            return Err(Error::internal("the write timed out"));
        }
        self.inner.complete(key, attempt, response, expires).await
    }

    async fn release(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
    ) -> crucible_server_runtime_rust::Result<Ownership> {
        self.inner.release(key, attempt).await
    }
}

#[tokio::test]
async fn a_lost_completion_keeps_the_success_and_holds_the_claim_for_the_retention() {
    let store = Arc::new(Faulty::default());
    store.fail_completions.store(true, Ordering::SeqCst);
    let clock = Clock::new();
    let idempotency = Idempotency::new(store.clone(), caller)
        .lease(Duration::from_secs(60))
        .clock(clock.reader());
    let harness = serve(idempotency).await;
    let first = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(first.status, 201, "the success stands");
    assert!(store.failed.load(Ordering::SeqCst));
    // Held for the retention, not the lease: long after the lease, a retry
    // is still refused rather than run again.
    clock.advance(Duration::from_secs(60 * 60));
    let retry = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(retry.status, 409);
    assert_eq!(harness.cities.creates(), 1);
}

#[tokio::test]
async fn a_response_too_large_to_keep_is_held_rather_than_replayed() {
    let idempotency = Idempotency::new(store(), caller).max_stored_response(16);
    let harness = serve(idempotency).await;
    assert_eq!(harness.create(Some("alice"), oslo("t-1")).await.status, 201);
    let retry = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(retry.status, 409);
    assert_eq!(harness.cities.creates(), 1);
}

#[tokio::test]
async fn a_response_from_before_a_deploy_is_a_conflict_not_a_failure() {
    let store = Arc::new(Faulty::default());
    let harness = serve(Idempotency::new(store.clone(), caller)).await;
    assert_eq!(harness.create(Some("alice"), oslo("t-1")).await.status, 201);
    store.stale.store(true, Ordering::SeqCst);
    let retry = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(retry.status, 409);
    let body = retry.json();
    assert_eq!(body["__type"], "ConflictException", "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("cannot be replayed"),
        "{body}"
    );
    assert_eq!(harness.cities.creates(), 1);
}

#[tokio::test]
async fn a_malformed_token_is_refused_naming_the_member() {
    let harness = serve(Idempotency::new(store(), caller)).await;
    for token in [
        "x".repeat(257),
        "with space".to_owned(),
        "caf\u{e9}".to_owned(),
    ] {
        let refused = harness.create(Some("alice"), oslo(&token)).await;
        assert_eq!(refused.status, 400, "{token:?}");
        let body = refused.json();
        assert_eq!(body["fieldList"][0]["path"], "clientToken", "{body}");
    }
    assert_eq!(harness.cities.creates(), 0);
}

#[tokio::test]
async fn an_attempt_that_outlived_its_claim_is_stopped_at_commit() {
    let clock = Clock::new();
    let idempotency = Idempotency::new(store(), caller)
        .lease(Duration::from_secs(60))
        .clock(clock.reader());
    let harness = Arc::new(serve(idempotency).await);
    let (entered, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    harness.cities.next(Step::WaitThenSend {
        entered: entered.clone(),
        release: release.clone(),
    });
    let slow = tokio::spawn({
        let harness = harness.clone();
        async move { harness.create(Some("alice"), oslo("t-1")).await }
    });
    entered.notified().await;
    // The first attempt runs past its lease; a retry takes the token over.
    clock.advance(Duration::from_secs(120));
    let retry = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(retry.status, 201);
    release.notify_one();
    let slow = slow.await.unwrap();
    // It no longer holds the token, so it stops before its irreversible step.
    assert_eq!(slow.status, 409);
    assert!(
        slow.json()["message"]
            .as_str()
            .unwrap()
            .contains("took longer"),
        "{}",
        slow.json()
    );
    assert_eq!(harness.cities.sends(), 0);
    assert_eq!(harness.cities.creates(), 2);
    // The retry's success is what replays.
    let again = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(again.body, retry.body);
}

#[tokio::test]
async fn a_completion_after_a_takeover_does_not_overwrite_the_record() {
    let clock = Clock::new();
    let idempotency = Idempotency::new(store(), caller)
        .lease(Duration::from_secs(60))
        .clock(clock.reader());
    let harness = Arc::new(serve(idempotency).await);
    let (entered, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    harness.cities.next(Step::Wait {
        entered: entered.clone(),
        release: release.clone(),
    });
    let slow = tokio::spawn({
        let harness = harness.clone();
        async move { harness.create(Some("alice"), oslo("t-1")).await }
    });
    entered.notified().await;
    clock.advance(Duration::from_secs(120));
    let retry = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(retry.status, 201);
    release.notify_one();
    let slow = slow.await.unwrap();
    // Its own success stands, but the record keeps the attempt that held it.
    assert_eq!(slow.status, 201);
    assert_ne!(slow.body, retry.body);
    let again = harness.create(Some("alice"), oslo("t-1")).await;
    assert_eq!(again.body, retry.body);
}

#[tokio::test]
async fn an_mcp_tool_call_reusing_a_token_gets_a_tool_error() {
    let cities = Arc::new(Counted::default());
    let service = weather_service::builder(cities.clone())
        .mcp(McpConfig::new())
        .interceptor(Idempotency::new(store(), |_| None))
        .build()
        .unwrap();
    let call = |arguments: Value| {
        let service = service.clone();
        async move {
            let message = json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "tools/call",
                "params": { "name": "CreateCity", "arguments": arguments },
            });
            let request = axum::http::Request::post("/mcp")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(axum::body::Body::from(message.to_string()))
                .unwrap();
            let response = service.handle(request).await;
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        }
    };
    let first = call(json!({ "name": "Oslo", "clientToken": "t-1" })).await;
    assert_eq!(first["result"]["isError"], false, "{first}");
    let replay = call(json!({ "name": "Oslo", "clientToken": "t-1" })).await;
    assert_eq!(
        replay["result"]["structuredContent"],
        first["result"]["structuredContent"]
    );
    let other = call(json!({ "name": "Bergen", "clientToken": "t-1" })).await;
    assert_eq!(other["result"]["isError"], true, "{other}");
    let error: Value =
        serde_json::from_str(other["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(error["name"], "ConflictException", "{error}");
    assert_eq!(cities.creates(), 1);
}
