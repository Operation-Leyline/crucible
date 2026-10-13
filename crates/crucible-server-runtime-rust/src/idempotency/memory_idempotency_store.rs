use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use super::{Claim, Fingerprint, IdempotencyKey, IdempotencyStore, Ownership, StoredResponse};
use crate::{Error, Result};

/// Idempotency records in memory, for tests: one process's records, gone
/// when it exits, so never for a deployment, where every instance must see
/// the same claims. A mutex makes each call atomic, with the semantics the
/// conditional writes [`IdempotencyStore`] describes for DynamoDB give.
#[derive(Default)]
pub struct MemoryIdempotencyStore {
    records: Mutex<HashMap<IdempotencyKey, Record>>,
    unavailable: AtomicBool,
}

struct Record {
    fingerprint: Fingerprint,
    attempt: String,
    expires: SystemTime,
    response: Option<StoredResponse>,
}

impl MemoryIdempotencyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// While `true`, every call fails, as a store outage would.
    pub fn set_unavailable(&self, unavailable: bool) {
        self.unavailable.store(unavailable, Ordering::SeqCst);
    }

    /// The records kept, expired ones included until a claim replaces them.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether `key`'s record is in flight (claimed, not completed), as of
    /// its last write, expired or not.
    pub fn in_flight(&self, key: &IdempotencyKey) -> bool {
        self.lock()
            .get(key)
            .is_some_and(|record| record.response.is_none())
    }

    /// Drops `key`'s record, as a store's TTL deletes an expired one: the
    /// attempt that held it then finds its claim lost.
    pub fn forget(&self, key: &IdempotencyKey) {
        self.lock().remove(key);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<IdempotencyKey, Record>> {
        // A panic while holding the lock leaves whole records behind.
        self.records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn available(&self) -> Result<()> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(Error::internal("the idempotency store is unavailable"));
        }
        Ok(())
    }

    /// `f` on `attempt`'s record of `key`, when it is still that attempt's
    /// in-flight record: the condition of a conditional write.
    fn with_attempt(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        f: impl FnOnce(&mut Record),
    ) -> Ownership {
        match self.lock().get_mut(key) {
            Some(record) if record.attempt == attempt && record.response.is_none() => {
                f(record);
                Ownership::Held
            }
            _ => Ownership::Lost,
        }
    }
}

#[async_trait::async_trait]
impl IdempotencyStore for MemoryIdempotencyStore {
    async fn claim(
        &self,
        key: &IdempotencyKey,
        fingerprint: &Fingerprint,
        attempt: &str,
        now: SystemTime,
        expires: SystemTime,
    ) -> Result<Claim> {
        self.available()?;
        let mut records = self.lock();
        if let Some(record) = records.get(key)
            && record.expires > now
        {
            return Ok(match &record.response {
                Some(response) => Claim::Replay {
                    fingerprint: record.fingerprint,
                    response: response.clone(),
                },
                None => Claim::InFlight {
                    fingerprint: record.fingerprint,
                },
            });
        }
        records.insert(
            key.clone(),
            Record {
                fingerprint: *fingerprint,
                attempt: attempt.to_owned(),
                expires,
                response: None,
            },
        );
        Ok(Claim::Fresh)
    }

    async fn hold(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        expires: SystemTime,
    ) -> Result<Ownership> {
        self.available()?;
        Ok(self.with_attempt(key, attempt, |record| record.expires = expires))
    }

    async fn complete(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        response: &StoredResponse,
        expires: SystemTime,
    ) -> Result<Ownership> {
        self.available()?;
        Ok(self.with_attempt(key, attempt, |record| {
            record.response = Some(response.clone());
            record.expires = expires;
        }))
    }

    async fn release(&self, key: &IdempotencyKey, attempt: &str) -> Result<Ownership> {
        self.available()?;
        let mut records = self.lock();
        if records
            .get(key)
            .is_some_and(|record| record.attempt == attempt && record.response.is_none())
        {
            records.remove(key);
            return Ok(Ownership::Held);
        }
        Ok(Ownership::Lost)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use serde_json::json;

    use super::MemoryIdempotencyStore;
    use crate::idempotency::{
        Claim, Fingerprint, IdempotencyKey, IdempotencyStore, Ownership, StoredResponse,
    };

    fn key(token: &str) -> IdempotencyKey {
        IdempotencyKey::new("s#S", "s#Create", Some("alice".to_owned()), token)
    }

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[tokio::test]
    async fn a_claim_is_fresh_once_then_in_flight_then_replayed() {
        let store = MemoryIdempotencyStore::new();
        let print = Fingerprint::of(&json!({"name": "Oslo"}));
        let first = store.claim(&key("t"), &print, "a1", at(0), at(60)).await;
        assert_eq!(first.unwrap(), Claim::Fresh);
        let second = store.claim(&key("t"), &print, "a2", at(1), at(61)).await;
        assert_eq!(second.unwrap(), Claim::InFlight { fingerprint: print });
        let response = StoredResponse::new(r#"{"city":{}}"#);
        let completed = store.complete(&key("t"), "a1", &response, at(100)).await;
        assert_eq!(completed.unwrap(), Ownership::Held);
        // A completed record is no attempt's claim any more.
        let late = store.release(&key("t"), "a1").await;
        assert_eq!(late.unwrap(), Ownership::Lost);
        let third = store.claim(&key("t"), &print, "a3", at(2), at(62)).await;
        assert_eq!(
            third.unwrap(),
            Claim::Replay {
                fingerprint: print,
                response
            }
        );
    }

    #[tokio::test]
    async fn an_expired_record_is_absent() {
        let store = MemoryIdempotencyStore::new();
        let print = Fingerprint::of(&json!({}));
        store
            .claim(&key("t"), &print, "a1", at(0), at(60))
            .await
            .unwrap();
        // Expiry is exclusive: at the instant it expires, it is gone.
        let later = store.claim(&key("t"), &print, "a2", at(60), at(120)).await;
        assert_eq!(later.unwrap(), Claim::Fresh);
        // The first attempt no longer holds it, so it cannot release the second's.
        let lost = store.release(&key("t"), "a1").await;
        assert_eq!(lost.unwrap(), Ownership::Lost);
        assert!(store.in_flight(&key("t")));
        let released = store.release(&key("t"), "a2").await;
        assert_eq!(released.unwrap(), Ownership::Held);
        assert!(store.is_empty());
    }

    #[tokio::test]
    async fn hold_extends_only_the_holders_claim() {
        let store = MemoryIdempotencyStore::new();
        let print = Fingerprint::of(&json!({}));
        store
            .claim(&key("t"), &print, "a1", at(0), at(60))
            .await
            .unwrap();
        let other = store.hold(&key("t"), "other", at(1000)).await;
        assert_eq!(other.unwrap(), Ownership::Lost);
        let expired = store.claim(&key("t"), &print, "a2", at(61), at(121)).await;
        assert_eq!(expired.unwrap(), Claim::Fresh);
        // The first attempt outlived its claim and another took it over.
        let lapsed = store.hold(&key("t"), "a1", at(1000)).await;
        assert_eq!(lapsed.unwrap(), Ownership::Lost);
        let held = store.hold(&key("t"), "a2", at(1000)).await;
        assert_eq!(held.unwrap(), Ownership::Held);
        let held = store.claim(&key("t"), &print, "a3", at(500), at(560)).await;
        assert_eq!(held.unwrap(), Claim::InFlight { fingerprint: print });
    }

    #[tokio::test]
    async fn a_completion_after_a_takeover_is_lost_and_overwrites_nothing() {
        let store = MemoryIdempotencyStore::new();
        let print = Fingerprint::of(&json!({}));
        store
            .claim(&key("t"), &print, "a1", at(0), at(60))
            .await
            .unwrap();
        store
            .claim(&key("t"), &print, "a2", at(61), at(121))
            .await
            .unwrap();
        let second = StoredResponse::new("2");
        let completed = store.complete(&key("t"), "a2", &second, at(500)).await;
        assert_eq!(completed.unwrap(), Ownership::Held);
        let first = store
            .complete(&key("t"), "a1", &StoredResponse::new("1"), at(500))
            .await;
        assert_eq!(first.unwrap(), Ownership::Lost);
        let replay = store.claim(&key("t"), &print, "a3", at(62), at(122)).await;
        assert_eq!(
            replay.unwrap(),
            Claim::Replay {
                fingerprint: print,
                response: second
            }
        );
        // A record a TTL deleted is lost to its attempt too.
        store
            .claim(&key("u"), &print, "a4", at(0), at(60))
            .await
            .unwrap();
        store.forget(&key("u"));
        let deleted = store.hold(&key("u"), "a4", at(500)).await;
        assert_eq!(deleted.unwrap(), Ownership::Lost);
    }

    #[tokio::test]
    async fn keys_are_independent_and_outages_fail() {
        let store = MemoryIdempotencyStore::new();
        let print = Fingerprint::of(&json!({}));
        let bob = IdempotencyKey::new("s#S", "s#Create", Some("bob".to_owned()), "t");
        for key in [key("t"), bob] {
            let claim = store.claim(&key, &print, "a", at(0), at(60)).await;
            assert_eq!(claim.unwrap(), Claim::Fresh);
        }
        store.set_unavailable(true);
        assert!(
            store
                .claim(&key("u"), &print, "a", at(0), at(60))
                .await
                .is_err()
        );
        assert!(store.release(&key("t"), "a").await.is_err());
    }
}
