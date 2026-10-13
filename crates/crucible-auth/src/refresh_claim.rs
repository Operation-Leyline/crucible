use crate::api_key_cache::ApiKeyCache;
use std::sync::Arc;

/// Releases a digest's refresh-ahead slot however the background read ends,
/// including when the runtime drops the task.
pub(crate) struct RefreshClaim {
    pub cache: Arc<ApiKeyCache>,
    pub digest: String,
}

impl Drop for RefreshClaim {
    fn drop(&mut self) {
        if let Ok(mut refreshing) = self.cache.refreshing.lock() {
            refreshing.remove(&self.digest);
        }
    }
}
