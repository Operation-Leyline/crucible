#![cfg(feature = "test-support")]
//! `ApiKeyAuthenticator` caching, recognition and hooks, against the in-memory
//! store with an injected clock: no test waits for real time to pass.
use axum::http::{HeaderName, Request, request::Parts};
use crucible_auth::{
    ApiKeyAuthenticator, ApiKeyRecord, ApiKeyState, ApiKeyStore, ApiKeyUsageRecorder,
    AuthenticateResult, Authenticator, TokenKind, api_key_digest,
    test_support::InMemoryApiKeyStore,
};
use crucible_server_runtime_rust::operation::AuthScheme;
use serde_json::json;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const KEY: &str = "pgn_live_0123456789";

#[derive(Clone)]
struct TestClock(Arc<Mutex<SystemTime>>);

impl TestClock {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(
            UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        )))
    }
    fn now(&self) -> SystemTime {
        *self.0.lock().unwrap()
    }
    fn advance(&self, seconds: u64) {
        *self.0.lock().unwrap() += Duration::from_secs(seconds);
    }
}

fn active(id: &str) -> ApiKeyRecord {
    ApiKeyRecord::new(id, "workspace-1", ApiKeyState::Active)
}

fn setup() -> (InMemoryApiKeyStore, TestClock, ApiKeyAuthenticator) {
    let store = InMemoryApiKeyStore::new();
    let clock = TestClock::new();
    let now = clock.clone();
    let auth = ApiKeyAuthenticator::new("pgn_", store.clone())
        .unwrap()
        .clock(move || now.now());
    (store, clock, auth)
}

fn parts(headers: &[(&str, &str)]) -> Parts {
    let mut request = Request::get("/");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.body(()).unwrap().into_parts().0
}

fn bearer(key: &str) -> Parts {
    parts(&[("authorization", &format!("Bearer {key}"))])
}

async fn check(auth: &ApiKeyAuthenticator, key: &str) -> AuthenticateResult {
    auth.authenticate(&bearer(key), &AuthScheme::Bearer).await
}

fn admitted(result: &AuthenticateResult) -> bool {
    matches!(result, AuthenticateResult::Authenticated(_))
}

fn refused(result: &AuthenticateResult) -> bool {
    matches!(result, AuthenticateResult::Refused)
}

#[test]
fn prefix_must_be_nonempty_without_spaces() {
    for prefix in ["", "pg n_", "pgn\t"] {
        assert!(ApiKeyAuthenticator::new(prefix, InMemoryApiKeyStore::new()).is_err());
    }
}

#[test]
fn inconsistent_configuration_is_an_error() {
    let auth = || ApiKeyAuthenticator::new("pgn_", InMemoryApiKeyStore::new()).unwrap();
    let second = Duration::from_secs(1);
    assert!(auth().credential_headers([]).is_err());
    assert!(auth().refusal_backoff(Duration::ZERO, second).is_err());
    assert!(auth().refusal_backoff(second * 2, second).is_err());
    assert!(auth().refusal_backoff(second, Duration::ZERO).is_err());
    assert!(auth().refusal_backoff(second, second).is_ok());
    assert!(
        auth()
            .refusal_backoff(Duration::ZERO, Duration::ZERO)
            .is_ok()
    );
    assert!(auth().lookup_timeout(Duration::ZERO).is_err());
    assert!(auth().lookup_timeout(second).is_ok());
    assert!(auth().last_used_timeout(Duration::ZERO).is_err());
    assert!(auth().last_used_timeout(second).is_ok());
}

#[test]
fn digest_is_lowercase_hex_sha256_of_the_whole_credential() {
    assert_eq!(
        api_key_digest("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let digest = api_key_digest(KEY);
    assert_eq!(digest.len(), 64);
    assert!(
        digest
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    );
    assert_ne!(digest, api_key_digest(&KEY["pgn_".len()..]));
    assert_eq!(InMemoryApiKeyStore::new().insert(KEY, active("k")), digest);
}

#[tokio::test]
async fn only_recognized_credentials_are_claimed_or_read() {
    let (store, _, auth) = setup();
    let auth = auth.well_formed(|key| key.ends_with("789"));
    let scheme = AuthScheme::Bearer;
    assert!(auth.supports(&AuthScheme::Bearer));
    assert!(auth.has_credentials(&bearer(KEY), &scheme));
    for value in [
        "Bearer eyJhbGciOiJSUzI1NiJ9.e30.sig",
        "Bearer pgn_",
        "Bearer pgn_live_0123456780",
        "Bearer xpgn_live_0123456789",
        "Basic pgn_live_0123456789",
        "Bearer pgn_live 0123456789",
        "Bearer",
    ] {
        let request = parts(&[("authorization", value)]);
        assert!(!auth.has_credentials(&request, &scheme), "{value}");
        assert!(refused(&auth.authenticate(&request, &scheme).await));
    }
    let long = format!("Bearer pgn_{}789", "a".repeat(1100));
    assert!(!auth.has_credentials(&parts(&[("authorization", &long)]), &scheme));
    assert!(!auth.has_credentials(&parts(&[]), &scheme));
    assert_eq!(store.lookups(), 0, "unrecognized credentials cost no read");
}

#[tokio::test]
async fn bearer_scheme_bare_keys_and_duplicate_headers() {
    let (store, _, auth) = setup();
    store.insert(KEY, active("key-1"));
    let scheme = AuthScheme::Bearer;
    let lower = parts(&[("authorization", &format!("bEaReR {KEY}"))]);
    assert!(admitted(&auth.authenticate(&lower, &scheme).await));
    let bare = parts(&[("authorization", KEY)]);
    assert!(
        !auth.has_credentials(&bare, &scheme),
        "bare keys are off by default"
    );
    let duplicated = parts(&[
        ("authorization", &format!("Bearer {KEY}")),
        ("authorization", "Bearer other"),
    ]);
    assert!(
        auth.has_credentials(&duplicated, &scheme),
        "a duplicated key is refused, never treated as absent"
    );
    assert!(refused(&auth.authenticate(&duplicated, &scheme).await));
    let foreign = parts(&[("authorization", "Bearer a"), ("authorization", "Bearer b")]);
    assert!(!auth.has_credentials(&foreign, &scheme));
    let auth = auth.allow_bare(true);
    assert!(admitted(&auth.authenticate(&bare, &scheme).await));
    assert!(admitted(&auth.authenticate(&lower, &scheme).await));
}

#[tokio::test]
async fn first_present_header_wins() {
    let (store, _, auth) = setup();
    store.insert(KEY, active("key-1"));
    let auth = auth
        .credential_headers([
            HeaderName::from_static("x-leyline-authorization"),
            HeaderName::from_static("authorization"),
        ])
        .unwrap();
    let scheme = AuthScheme::Bearer;
    let key = format!("Bearer {KEY}");
    let custom = parts(&[
        ("x-leyline-authorization", &key),
        ("authorization", "AWS4-HMAC-SHA256 oac"),
    ]);
    assert!(admitted(&auth.authenticate(&custom, &scheme).await));
    let fallback = parts(&[("authorization", &key)]);
    assert!(admitted(&auth.authenticate(&fallback, &scheme).await));
    let shadowed = parts(&[
        ("x-leyline-authorization", "Bearer eyJ.jwt.sig"),
        ("authorization", &key),
    ]);
    assert!(!auth.has_credentials(&shadowed, &scheme));
}

#[tokio::test]
async fn principal_carries_key_id_tenant_and_attributes() {
    let (store, clock, auth) = setup();
    let auth = auth.issuer("pigeon-keys");
    store.insert(
        KEY,
        active("key-1")
            .with_expiry(clock.now() + Duration::from_secs(3600))
            .with_attributes(json!({"name": "ci", "rate": 10})),
    );
    let AuthenticateResult::Authenticated(principal) = check(&auth, KEY).await else {
        panic!("active key refused");
    };
    assert_eq!(principal.subject, "key-1");
    assert_eq!(principal.tenant.as_deref(), Some("workspace-1"));
    assert_eq!(principal.issuer, "pigeon-keys");
    assert_eq!(principal.token_kind, TokenKind::ApiKey);
    assert_eq!(principal.client_id, None);
    assert!(principal.groups.is_empty());
    assert_eq!(principal.claims, json!({"name": "ci", "rate": 10}));
}

#[tokio::test]
async fn revoked_disabled_expired_and_unknown_keys_are_refused() {
    let (store, clock, auth) = setup();
    let cases = [
        (
            "pgn_revoked",
            ApiKeyRecord::new("r", "t", ApiKeyState::Revoked),
        ),
        (
            "pgn_disabled",
            ApiKeyRecord::new("d", "t", ApiKeyState::Disabled),
        ),
        ("pgn_expired", active("e").with_expiry(clock.now())),
    ];
    for (key, record) in cases {
        store.insert(key, record);
        assert!(refused(&check(&auth, key).await), "{key}");
    }
    assert!(refused(&check(&auth, "pgn_unknown").await));
}

#[tokio::test]
async fn expiry_is_rechecked_on_cache_hits() {
    let (store, clock, auth) = setup();
    store.insert(
        KEY,
        active("key-1").with_expiry(clock.now() + Duration::from_secs(60)),
    );
    assert!(admitted(&check(&auth, KEY).await));
    clock.advance(59);
    assert!(admitted(&check(&auth, KEY).await));
    clock.advance(1);
    assert!(refused(&check(&auth, KEY).await));
    assert_eq!(
        store.lookups(),
        1,
        "the cached record's own expiry refused it"
    );
}

#[tokio::test]
async fn a_valid_key_is_read_once_per_ttl_from_its_read() {
    let (store, clock, auth) = setup();
    let auth = auth.refresh_ahead(Duration::ZERO);
    store.insert(KEY, active("key-1"));
    for _ in 0..3 {
        assert!(admitted(&check(&auth, KEY).await));
    }
    clock.advance(299);
    assert!(admitted(&check(&auth, KEY).await));
    assert_eq!(store.lookups(), 1);
    // The hit at 299 s did not extend the entry.
    clock.advance(1);
    assert!(admitted(&check(&auth, KEY).await));
    assert_eq!(store.lookups(), 2);
    // A revocation elsewhere is seen within the TTL.
    store.revoke(KEY);
    clock.advance(299);
    assert!(admitted(&check(&auth, KEY).await));
    clock.advance(1);
    assert!(refused(&check(&auth, KEY).await));
}

async fn until_lookups(store: &InMemoryApiKeyStore, expected: usize) {
    for _ in 0..1000 {
        if store.lookups() >= expected {
            break;
        }
        tokio::task::yield_now().await;
    }
    // Let the refresh finish applying what it read.
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(store.lookups(), expected);
}

#[tokio::test]
async fn refresh_ahead_rereads_once_in_the_background_and_refreshes() {
    let (store, clock, auth) = setup();
    store.insert(KEY, active("key-1"));
    assert!(admitted(&check(&auth, KEY).await));
    clock.advance(239);
    assert!(admitted(&check(&auth, KEY).await));
    tokio::task::yield_now().await;
    assert_eq!(store.lookups(), 1, "before the last fifth: no refresh");
    clock.advance(1);
    // Answered from cache at once; several hits start one re-read.
    for _ in 0..5 {
        assert!(admitted(&check(&auth, KEY).await));
    }
    until_lookups(&store, 2).await;
    // The entry now dates from the refresh at 240 s, so 470 s is still fresh.
    clock.advance(230);
    assert!(admitted(&check(&auth, KEY).await));
    tokio::task::yield_now().await;
    assert_eq!(store.lookups(), 2);
}

#[tokio::test]
async fn refresh_ahead_evicts_a_key_revoked_since_it_was_read() {
    let (store, clock, auth) = setup();
    store.insert(KEY, active("key-1"));
    assert!(admitted(&check(&auth, KEY).await));
    store.revoke(KEY);
    clock.advance(250);
    assert!(
        admitted(&check(&auth, KEY).await),
        "the hit that triggers the refresh is answered from cache"
    );
    until_lookups(&store, 2).await;
    assert!(refused(&check(&auth, KEY).await));
    assert_eq!(store.lookups(), 2, "the dead row is a cached refusal");
}

/// Reads happen exactly when the previous refusal's window has run out.
async fn refusal_windows(
    auth: &ApiKeyAuthenticator,
    store: &InMemoryApiKeyStore,
    clock: &TestClock,
    key: &str,
) -> Vec<u64> {
    let before = store.lookups();
    assert!(refused(&check(auth, key).await));
    assert_eq!(store.lookups(), before + 1, "the first refusal reads");
    let mut windows = Vec::new();
    for _ in 0..7 {
        let mut waited = 0;
        loop {
            clock.advance(1);
            waited += 1;
            assert!(waited <= 60, "refusal outlived the ceiling");
            let reads = store.lookups();
            // A cached refusal leaves the count alone; the read that ends a
            // window starts the next one.
            assert!(refused(&check(auth, key).await));
            if store.lookups() > reads {
                windows.push(waited);
                break;
            }
        }
    }
    windows
}

#[tokio::test]
async fn unknown_keys_back_off_and_dead_rows_go_to_the_ceiling() {
    let (store, clock, auth) = setup();
    // Each window ends with the probe that read; the next window starts there.
    let windows = refusal_windows(&auth, &store, &clock, "pgn_forged").await;
    assert_eq!(windows, [2, 4, 8, 16, 32, 60, 60]);

    store.insert(KEY, ApiKeyRecord::new("dead", "t", ApiKeyState::Revoked));
    let windows = refusal_windows(&auth, &store, &clock, KEY).await;
    assert_eq!(windows, [60; 7]);
}

#[tokio::test]
async fn a_valid_read_clears_a_refusal_and_a_refusal_clears_a_valid_entry() {
    let (store, clock, auth) = setup();
    assert!(refused(&check(&auth, KEY).await));
    store.insert(KEY, active("key-1"));
    assert!(refused(&check(&auth, KEY).await), "still within 2 s");
    clock.advance(2);
    assert!(admitted(&check(&auth, KEY).await));
    // The refusal history is gone: a later miss starts again at 2 s.
    store.remove(KEY);
    auth.forget_credential(KEY);
    let windows = refusal_windows(&auth, &store, &clock, KEY).await;
    assert_eq!(windows[0], 2);
}

#[tokio::test]
async fn forged_key_floods_cannot_evict_valid_keys() {
    let (store, _, auth) = setup();
    let auth = auth.valid_capacity(2).refused_capacity(4);
    store.insert(KEY, active("key-1"));
    assert!(admitted(&check(&auth, KEY).await));
    for n in 0..100 {
        assert!(refused(&check(&auth, &format!("pgn_forged_{n}")).await));
    }
    assert!(admitted(&check(&auth, KEY).await));
    assert_eq!(store.lookups(), 101);
}

#[tokio::test]
async fn zero_valid_ttl_reads_every_valid_request_but_still_caches_refusals() {
    let (store, _, auth) = setup();
    let auth = auth.valid_ttl(Duration::ZERO);
    store.insert(KEY, active("key-1"));
    assert!(admitted(&check(&auth, KEY).await));
    assert!(admitted(&check(&auth, KEY).await));
    assert_eq!(
        store.lookups(),
        2,
        "immediate revocation: every request reads"
    );
    store.revoke(KEY);
    assert!(refused(&check(&auth, KEY).await));
    assert!(refused(&check(&auth, "pgn_forged").await));
    assert!(refused(&check(&auth, "pgn_forged").await));
    assert_eq!(
        store.lookups(),
        4,
        "a repeated forged key still costs one read"
    );
}

#[tokio::test]
async fn both_caches_off_caches_nothing() {
    let (store, _, auth) = setup();
    let auth = auth
        .valid_ttl(Duration::ZERO)
        .refusal_backoff(Duration::ZERO, Duration::ZERO)
        .unwrap();
    store.insert(KEY, active("key-1"));
    for key in [KEY, KEY, "pgn_forged", "pgn_forged"] {
        check(&auth, key).await;
    }
    assert_eq!(store.lookups(), 4);
}

#[tokio::test]
async fn an_outage_is_unavailable_and_never_cached() {
    let (store, _, auth) = setup();
    store.insert(KEY, active("key-1"));
    store.set_unavailable(true);
    let result = check(&auth, KEY).await;
    let AuthenticateResult::Unavailable(error) = result else {
        panic!("an outage must not be a refusal: {result:?}");
    };
    let chain = error.chain();
    assert!(!chain.contains(KEY) && !chain.contains(&api_key_digest(KEY)));
    store.set_unavailable(false);
    assert!(admitted(&check(&auth, KEY).await));
    assert_eq!(store.lookups(), 2);
}

fn unavailable(result: &AuthenticateResult) -> bool {
    matches!(result, AuthenticateResult::Unavailable(_))
}

#[tokio::test]
async fn a_fence_is_an_outage_for_every_key_without_reading() {
    let (store, _, auth) = setup();
    let fenced = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&fenced);
    let auth = auth.fence(move || flag.load(Ordering::SeqCst));
    store.insert(KEY, active("key-1"));
    assert!(admitted(&check(&auth, KEY).await));
    fenced.store(true, Ordering::SeqCst);
    // A valid key is never told it is invalid because a replica is degraded.
    assert!(unavailable(&check(&auth, KEY).await), "cached keys too");
    assert!(unavailable(&check(&auth, "pgn_other").await));
    assert_eq!(store.lookups(), 1);
    fenced.store(false, Ordering::SeqCst);
    assert!(admitted(&check(&auth, KEY).await));
}

#[tokio::test]
async fn forget_drops_both_caches_for_a_digest() {
    let (store, _, auth) = setup();
    store.insert(KEY, active("key-1"));
    assert!(admitted(&check(&auth, KEY).await));
    store.revoke(KEY);
    assert!(admitted(&check(&auth, KEY).await), "cached until forgotten");
    auth.forget(&api_key_digest(KEY));
    assert!(refused(&check(&auth, KEY).await));
    store.insert(KEY, active("key-1"));
    assert!(refused(&check(&auth, KEY).await), "refusal cached");
    auth.forget_credential(KEY);
    assert!(admitted(&check(&auth, KEY).await));
    assert_eq!(store.lookups(), 3);
}

struct Uses(Arc<Mutex<Vec<(String, String)>>>);

#[async_trait::async_trait]
impl ApiKeyUsageRecorder for Uses {
    async fn record_use(&self, digest: &str, record: &ApiKeyRecord) {
        self.0
            .lock()
            .unwrap()
            .push((digest.to_owned(), record.key_id.clone()));
    }
}

#[tokio::test]
async fn last_used_is_recorded_once_per_resolution() {
    let (store, clock, auth) = setup();
    let uses = Arc::new(Mutex::new(Vec::new()));
    let auth = auth
        .valid_ttl(Duration::ZERO)
        .last_used(Uses(Arc::clone(&uses)));
    store.insert(KEY, active("key-1"));
    for _ in 0..3 {
        assert!(admitted(&check(&auth, KEY).await));
    }
    assert_eq!(
        *uses.lock().unwrap(),
        [(api_key_digest(KEY), "key-1".to_owned())]
    );
    clock.advance(3599);
    assert!(admitted(&check(&auth, KEY).await));
    assert_eq!(uses.lock().unwrap().len(), 1);
    clock.advance(1);
    assert!(admitted(&check(&auth, KEY).await));
    assert_eq!(uses.lock().unwrap().len(), 2);
    assert!(refused(&check(&auth, "pgn_forged").await));
    assert_eq!(uses.lock().unwrap().len(), 2, "refusals are not uses");
}

/// What a `GatedStore` read is doing, in order, so a test can wait for a step
/// instead of counting yields.
#[derive(Debug, PartialEq)]
enum Step {
    /// The row was read (the snapshot is fixed) and the reply is held.
    Entered,
    /// The held reply was released to the authenticator.
    Left,
}

/// Reads the row at once, then holds the reply until released: a slow store
/// whose answer describes the row as it was when the read began. Records how
/// many replies were held at once.
#[derive(Clone)]
struct GatedStore {
    inner: InMemoryApiKeyStore,
    gate: Arc<tokio::sync::Semaphore>,
    running: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    steps: tokio::sync::mpsc::UnboundedSender<Step>,
}

impl GatedStore {
    fn new(inner: InMemoryApiKeyStore) -> (Self, tokio::sync::mpsc::UnboundedReceiver<Step>) {
        let (steps, receiver) = tokio::sync::mpsc::unbounded_channel();
        let store = Self {
            inner,
            gate: Arc::new(tokio::sync::Semaphore::new(0)),
            running: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
            steps,
        };
        (store, receiver)
    }

    fn release(&self, reads: usize) {
        self.gate.add_permits(reads);
    }
}

#[async_trait::async_trait]
impl ApiKeyStore for GatedStore {
    async fn lookup(
        &self,
        digest: &str,
    ) -> Result<Option<ApiKeyRecord>, crucible_server_runtime_rust::Error> {
        let snapshot = self.inner.lookup(digest).await;
        let running = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(running, Ordering::SeqCst);
        let _ = self.steps.send(Step::Entered);
        self.gate.acquire().await.unwrap().forget();
        self.running.fetch_sub(1, Ordering::SeqCst);
        let _ = self.steps.send(Step::Left);
        snapshot
    }
}

async fn expect(steps: &mut tokio::sync::mpsc::UnboundedReceiver<Step>, step: Step) {
    assert_eq!(steps.recv().await, Some(step));
}

fn gated(
    store: &InMemoryApiKeyStore,
    clock: &TestClock,
) -> (
    GatedStore,
    tokio::sync::mpsc::UnboundedReceiver<Step>,
    Arc<ApiKeyAuthenticator>,
) {
    let (gated, steps) = GatedStore::new(store.clone());
    let now = clock.clone();
    let auth = ApiKeyAuthenticator::new("pgn_", gated.clone())
        .unwrap()
        .clock(move || now.now());
    (gated, steps, Arc::new(auth))
}

#[tokio::test]
async fn concurrent_misses_for_one_key_share_one_lookup() {
    let store = InMemoryApiKeyStore::new();
    store.insert(KEY, active("key-1"));
    let (gated, mut steps, auth) = gated(&store, &TestClock::new());
    // No valid cache, so only single flight can explain one read.
    let auth = Arc::new(Arc::into_inner(auth).unwrap().valid_ttl(Duration::ZERO));
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..20 {
        let auth = Arc::clone(&auth);
        tasks.spawn(async move { admitted(&check(&auth, KEY).await) });
    }
    expect(&mut steps, Step::Entered).await;
    gated.release(100);
    while let Some(result) = tasks.join_next().await {
        assert!(result.unwrap());
    }
    assert_eq!(store.lookups(), 1);
    assert_eq!(gated.peak.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn store_reads_are_bounded_by_the_lookup_limit() {
    let store = InMemoryApiKeyStore::new();
    let (gated, mut steps, auth) = gated(&store, &TestClock::new());
    let auth = Arc::new(Arc::into_inner(auth).unwrap().lookup_concurrency(2));
    let mut tasks = tokio::task::JoinSet::new();
    for n in 0..10 {
        let auth = Arc::clone(&auth);
        tasks.spawn(async move { refused(&check(&auth, &format!("pgn_key_{n}")).await) });
    }
    expect(&mut steps, Step::Entered).await;
    expect(&mut steps, Step::Entered).await;
    // Each released read lets exactly one more in.
    for _ in 0..8 {
        gated.release(1);
        expect(&mut steps, Step::Left).await;
        expect(&mut steps, Step::Entered).await;
        assert!(gated.running.load(Ordering::SeqCst) <= 2);
    }
    gated.release(100);
    while let Some(result) = tasks.join_next().await {
        assert!(result.unwrap());
    }
    assert_eq!(gated.peak.load(Ordering::SeqCst), 2);
    assert_eq!(store.lookups(), 10);
}

#[tokio::test]
async fn a_read_in_flight_during_forget_is_not_cached() {
    let store = InMemoryApiKeyStore::new();
    store.insert(KEY, active("key-1"));
    let clock = TestClock::new();
    let (gated, mut steps, auth) = gated(&store, &clock);
    let reader = Arc::clone(&auth);
    let task = tokio::spawn(async move { admitted(&check(&reader, KEY).await) });
    expect(&mut steps, Step::Entered).await;
    store.revoke(KEY);
    auth.forget_credential(KEY);
    gated.release(100);
    // The read began before the revocation, so it answers with the old row...
    assert!(task.await.unwrap());
    // ...but did not cache it.
    assert!(refused(&check(&auth, KEY).await));
    assert_eq!(store.lookups(), 2);
}

#[tokio::test]
async fn a_slow_read_older_than_the_ttl_is_not_served() {
    let store = InMemoryApiKeyStore::new();
    store.insert(KEY, active("key-1"));
    let clock = TestClock::new();
    let (gated, mut steps, auth) = gated(&store, &clock);
    let reader = Arc::clone(&auth);
    let task = tokio::spawn(async move { check(&reader, KEY).await });
    expect(&mut steps, Step::Entered).await;
    // Revoked while the reply (still saying active) is held for over an hour.
    store.revoke(KEY);
    clock.advance(3800);
    gated.release(100);
    let result = task.await.unwrap();
    assert!(refused(&result), "the stale reply is re-read: {result:?}");
    assert_eq!(store.lookups(), 2);
    assert!(refused(&check(&auth, KEY).await));
}

#[tokio::test]
async fn a_request_joining_a_refresh_paused_past_the_ttl_reads_again() {
    let store = InMemoryApiKeyStore::new();
    store.insert(KEY, active("key-1"));
    let clock = TestClock::new();
    let (gated, mut steps, auth) = gated(&store, &clock);
    gated.release(1);
    assert!(admitted(&check(&auth, KEY).await));
    expect(&mut steps, Step::Entered).await;
    expect(&mut steps, Step::Left).await;
    clock.advance(250);
    assert!(admitted(&check(&auth, KEY).await));
    // The refresh-ahead read takes its snapshot, then the process "freezes".
    expect(&mut steps, Step::Entered).await;
    store.revoke(KEY);
    clock.advance(3800);
    // The entry has expired, so this request joins the paused refresh (polled
    // first, so it is waiting before the refresh is released).
    let (result, ()) = tokio::join!(check(&auth, KEY), async { gated.release(100) });
    assert!(
        refused(&result),
        "the old snapshot is not served: {result:?}"
    );
}

#[tokio::test]
async fn a_refresh_racing_forget_cannot_cache_the_old_row() {
    let store = InMemoryApiKeyStore::new();
    store.insert(KEY, active("key-1"));
    let clock = TestClock::new();
    let (gated, mut steps, auth) = gated(&store, &clock);
    gated.release(1);
    assert!(admitted(&check(&auth, KEY).await));
    expect(&mut steps, Step::Entered).await;
    expect(&mut steps, Step::Left).await;
    clock.advance(250);
    assert!(admitted(&check(&auth, KEY).await));
    expect(&mut steps, Step::Entered).await;
    store.revoke(KEY);
    auth.forget_credential(KEY);
    gated.release(100);
    assert!(refused(&check(&auth, KEY).await));
    // Both held replies are out: the request's and the refresh's (old row).
    expect(&mut steps, Step::Entered).await;
    expect(&mut steps, Step::Left).await;
    expect(&mut steps, Step::Left).await;
    tokio::task::yield_now().await;
    assert!(refused(&check(&auth, KEY).await));
    assert_eq!(store.lookups(), 3);
}

#[tokio::test]
async fn a_failed_refresh_keeps_the_entry_until_its_ttl() {
    let store = InMemoryApiKeyStore::new();
    store.insert(KEY, active("key-1"));
    let clock = TestClock::new();
    let (gated, mut steps, auth) = gated(&store, &clock);
    gated.release(1000);
    assert!(admitted(&check(&auth, KEY).await));
    expect(&mut steps, Step::Entered).await;
    expect(&mut steps, Step::Left).await;
    clock.advance(250);
    store.set_unavailable(true);
    assert!(admitted(&check(&auth, KEY).await));
    expect(&mut steps, Step::Entered).await;
    expect(&mut steps, Step::Left).await;
    tokio::task::yield_now().await;
    clock.advance(49);
    assert!(admitted(&check(&auth, KEY).await), "still within its TTL");
    clock.advance(1);
    assert!(
        unavailable(&check(&auth, KEY).await),
        "past its TTL the outage shows, as a 5xx and not a refusal"
    );
}

#[tokio::test]
async fn an_active_record_without_a_key_id_or_tenant_is_an_outage() {
    let (store, _, auth) = setup();
    for (n, (key_id, tenant)) in [("key-1", ""), ("", "workspace-1"), ("key-1", " ")]
        .into_iter()
        .enumerate()
    {
        let key = format!("pgn_blank_{n}");
        store.insert(&key, ApiKeyRecord::new(key_id, tenant, ApiKeyState::Active));
        // Never a principal with `tenant: Some("")`, and nothing cached.
        assert!(
            unavailable(&check(&auth, &key).await),
            "{key_id:?}/{tenant:?}"
        );
        assert!(unavailable(&check(&auth, &key).await));
    }
    assert_eq!(store.lookups(), 6);
    // A dead row is refused as usual, tenant or not.
    store.insert(
        "pgn_blank_revoked",
        ApiKeyRecord::new("key-2", "", ApiKeyState::Revoked),
    );
    assert!(refused(&check(&auth, "pgn_blank_revoked").await));
}

/// Never answers for the digests in `hangs`, like a store whose connection
/// to them hangs; reads everything else from `inner`. Counts the reads that
/// began.
#[derive(Clone)]
struct HangingStore {
    inner: InMemoryApiKeyStore,
    hangs: Arc<Mutex<Vec<String>>>,
    entered: Arc<AtomicUsize>,
}

impl HangingStore {
    fn new(inner: &InMemoryApiKeyStore, hangs: &[&str]) -> Self {
        Self {
            inner: inner.clone(),
            hangs: Arc::new(Mutex::new(
                hangs.iter().map(|key| api_key_digest(key)).collect(),
            )),
            entered: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait::async_trait]
impl ApiKeyStore for HangingStore {
    async fn lookup(
        &self,
        digest: &str,
    ) -> Result<Option<ApiKeyRecord>, crucible_server_runtime_rust::Error> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        if self.hangs.lock().unwrap().iter().any(|hung| hung == digest) {
            std::future::pending::<()>().await;
        }
        self.inner.lookup(digest).await
    }
}

#[tokio::test(start_paused = true)]
async fn a_hung_store_read_is_an_outage_at_its_deadline_and_frees_its_slot() {
    let store = InMemoryApiKeyStore::new();
    store.insert(KEY, active("key-1"));
    let hanging = HangingStore::new(&store, &["pgn_hangs"]);
    let auth = Arc::new(
        ApiKeyAuthenticator::new("pgn_", hanging.clone())
            .unwrap()
            .lookup_concurrency(1),
    );
    let started = tokio::time::Instant::now();
    let reader = Arc::clone(&auth);
    let hung = tokio::spawn(async move { check(&reader, "pgn_hangs").await });
    while hanging.entered.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    // Queued behind the hung read for the only slot: it fails at the same
    // deadline rather than waiting for the slot without bound, and never reads.
    let queued = check(&auth, KEY).await;
    assert!(unavailable(&queued), "{queued:?}");
    assert_eq!(hanging.entered.load(Ordering::SeqCst), 1);
    assert!(
        unavailable(&hung.await.unwrap()),
        "an outage, not a refusal"
    );
    assert_eq!(started.elapsed(), Duration::from_secs(5));
    // The hung read gave its slot back, and its outage was not cached.
    assert!(admitted(&check(&auth, KEY).await));
}

#[tokio::test(start_paused = true)]
async fn the_lookup_deadline_is_configurable() {
    let store = InMemoryApiKeyStore::new();
    let auth = ApiKeyAuthenticator::new("pgn_", HangingStore::new(&store, &["pgn_hangs"]))
        .unwrap()
        .lookup_timeout(Duration::from_millis(250))
        .unwrap();
    let started = tokio::time::Instant::now();
    assert!(unavailable(&check(&auth, "pgn_hangs").await));
    assert_eq!(started.elapsed(), Duration::from_millis(250));
}

#[tokio::test]
async fn refresh_ahead_has_its_own_slots_so_a_flood_of_misses_cannot_starve_it() {
    let store = InMemoryApiKeyStore::new();
    store.insert(KEY, active("key-1"));
    let clock = TestClock::new();
    let now = clock.clone();
    let hanging = HangingStore::new(&store, &["pgn_forged"]);
    let auth = Arc::new(
        ApiKeyAuthenticator::new("pgn_", hanging.clone())
            .unwrap()
            .lookup_concurrency(1)
            .clock(move || now.now()),
    );
    assert!(admitted(&check(&auth, KEY).await));
    // A forged key's read holds the only slot for misses.
    let reader = Arc::clone(&auth);
    let flood = tokio::spawn(async move { check(&reader, "pgn_forged").await });
    while hanging.entered.load(Ordering::SeqCst) < 2 {
        tokio::task::yield_now().await;
    }
    clock.advance(240);
    assert!(admitted(&check(&auth, KEY).await), "answered from cache");
    // The refresh-ahead read still runs.
    until_lookups(&store, 2).await;
    flood.abort();
}

/// Never returns, like a last-used write to a hung table.
struct HangingRecorder(Arc<AtomicUsize>);

#[async_trait::async_trait]
impl ApiKeyUsageRecorder for HangingRecorder {
    async fn record_use(&self, _: &str, _: &ApiKeyRecord) {
        self.0.fetch_add(1, Ordering::SeqCst);
        std::future::pending::<()>().await;
    }
}

#[tokio::test(start_paused = true)]
async fn a_hung_last_used_write_holds_the_request_only_until_its_deadline() {
    let (store, clock, auth) = setup();
    let calls = Arc::new(AtomicUsize::new(0));
    let auth = auth
        .last_used(HangingRecorder(Arc::clone(&calls)))
        .last_used_timeout(Duration::from_millis(300))
        .unwrap();
    store.insert(KEY, active("key-1"));
    let started = tokio::time::Instant::now();
    assert!(admitted(&check(&auth, KEY).await));
    assert_eq!(started.elapsed(), Duration::from_millis(300));
    // Cut short counts as a failed write: not retried within the window.
    assert!(admitted(&check(&auth, KEY).await));
    assert_eq!(started.elapsed(), Duration::from_millis(300));
    clock.advance(3600);
    assert!(admitted(&check(&auth, KEY).await));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
