use crate::{ApiKeyRecord, ApiKeyState, ApiKeyStore, api_key_digest};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

/// An [`ApiKeyStore`] in memory, for testing a service without its database.
/// Clones share one table, so a test keeps a clone to insert, revoke and
/// count reads while the authenticator owns another. Keys are stored by
/// [`api_key_digest`], as a real store would hold them.
#[derive(Clone, Default)]
pub struct InMemoryApiKeyStore {
    records: Arc<Mutex<HashMap<String, ApiKeyRecord>>>,
    lookups: Arc<AtomicUsize>,
    unavailable: Arc<AtomicBool>,
}

impl InMemoryApiKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `record` under the credential's digest and returns the digest.
    pub fn insert(&self, credential: &str, record: ApiKeyRecord) -> String {
        let digest = api_key_digest(credential);
        self.table().insert(digest.clone(), record);
        digest
    }

    /// Marks the key revoked; false when there is no such key.
    pub fn revoke(&self, credential: &str) -> bool {
        self.table()
            .get_mut(&api_key_digest(credential))
            .map(|record| record.state = ApiKeyState::Revoked)
            .is_some()
    }

    /// Deletes the key; false when there was no such key.
    pub fn remove(&self, credential: &str) -> bool {
        self.table().remove(&api_key_digest(credential)).is_some()
    }

    /// While set, every lookup fails as an unavailable store would.
    pub fn set_unavailable(&self, unavailable: bool) {
        self.unavailable.store(unavailable, Ordering::SeqCst);
    }

    /// Lookups served so far, including failed ones.
    pub fn lookups(&self) -> usize {
        self.lookups.load(Ordering::SeqCst)
    }

    fn table(&self) -> std::sync::MutexGuard<'_, HashMap<String, ApiKeyRecord>> {
        self.records.lock().expect("in-memory API key store lock")
    }
}

#[async_trait::async_trait]
impl ApiKeyStore for InMemoryApiKeyStore {
    async fn lookup(
        &self,
        digest: &str,
    ) -> Result<Option<ApiKeyRecord>, crucible_server_runtime_rust::Error> {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(crucible_server_runtime_rust::Error::internal(
                "in-memory API key store unavailable",
            ));
        }
        Ok(self.table().get(digest).cloned())
    }
}
