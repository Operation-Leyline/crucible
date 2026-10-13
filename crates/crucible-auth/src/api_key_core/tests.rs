//! Single-flight bookkeeping, which only the crate can observe: an abandoned
//! read must not leave its entry behind.
use super::ApiKeyCore;
use crate::{ApiKeyRecord, ApiKeyStore, api_key_cache::ApiKeyCache, api_key_policy::ApiKeyPolicy};
use std::sync::Arc;

/// Never answers, like a store whose connection hangs.
struct Hangs;

#[async_trait::async_trait]
impl ApiKeyStore for Hangs {
    async fn lookup(
        &self,
        _: &str,
    ) -> Result<Option<ApiKeyRecord>, crucible_server_runtime_rust::Error> {
        std::future::pending().await
    }
}

struct Panics;

#[async_trait::async_trait]
impl ApiKeyStore for Panics {
    async fn lookup(
        &self,
        _: &str,
    ) -> Result<Option<ApiKeyRecord>, crucible_server_runtime_rust::Error> {
        panic!("store bug")
    }
}

fn core(store: impl ApiKeyStore, concurrency: usize) -> ApiKeyCore {
    let mut policy = ApiKeyPolicy::new("pgn_".into());
    policy.lookup_concurrency = concurrency;
    ApiKeyCore {
        cache: Arc::new(ApiKeyCache::new(&policy)),
        policy: Arc::new(policy),
        store: Arc::new(store),
    }
}

fn request(core: &ApiKeyCore, key: &str) -> tokio::task::JoinHandle<()> {
    let core = core.clone();
    let key = key.to_owned();
    tokio::spawn(async move {
        core.authenticate(&key).await;
    })
}

/// Runs the other tasks until `entries` requests have joined reads. The
/// condition, not a yield count, decides when the step is done.
async fn until_inflight(core: &ApiKeyCore, entries: usize) {
    for _ in 0..10_000 {
        if core.inflight() == entries {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("inflight stayed at {} (wanted {entries})", core.inflight());
}

/// Requests holding the read for `key`'s digest (the map holds one more reference).
fn waiters(core: &ApiKeyCore, key: &str) -> usize {
    let inflight = core.cache.inflight.lock().unwrap();
    inflight
        .get(&crate::api_key_digest(key))
        .map_or(0, |cell| Arc::strong_count(cell) - 1)
}

async fn cancel(task: tokio::task::JoinHandle<()>) {
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn a_request_cancelled_during_its_read_leaves_no_entry() {
    let core = core(Hangs, 32);
    let tasks: Vec<_> = (0..100)
        .map(|n| request(&core, &format!("pgn_key_{n}")))
        .collect();
    until_inflight(&core, 100).await;
    for task in tasks {
        cancel(task).await;
    }
    assert_eq!(core.inflight(), 0);
}

#[tokio::test]
async fn a_request_cancelled_while_waiting_for_a_permit_leaves_no_entry() {
    let core = core(Hangs, 1);
    let reading = request(&core, "pgn_a");
    let queued = request(&core, "pgn_b");
    until_inflight(&core, 2).await;
    cancel(queued).await;
    assert_eq!(core.inflight(), 1);
    cancel(reading).await;
    assert_eq!(core.inflight(), 0);
}

#[tokio::test]
async fn the_entry_stays_while_another_request_still_waits() {
    let core = core(Hangs, 32);
    let first = request(&core, "pgn_a");
    until_inflight(&core, 1).await;
    let second = request(&core, "pgn_a");
    for _ in 0..10_000 {
        if waiters(&core, "pgn_a") == 2 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(waiters(&core, "pgn_a"), 2);
    cancel(first).await;
    assert_eq!(core.inflight(), 1, "the second request still waits on it");
    cancel(second).await;
    assert_eq!(core.inflight(), 0);
}

#[tokio::test]
async fn a_panicking_store_leaves_no_entry() {
    let core = core(Panics, 32);
    let error = request(&core, "pgn_a").await.unwrap_err();
    assert!(error.is_panic());
    assert_eq!(core.inflight(), 0);
}
