use crate::ApiKeyRecord;
use std::{sync::Arc, time::SystemTime};

/// A valid key as read from the store. Its lifetime runs from `read_at`; a hit
/// does not extend it, so the TTL bounds how stale a revocation can be.
pub(crate) struct CachedApiKey {
    pub record: Arc<ApiKeyRecord>,
    pub read_at: SystemTime,
}
