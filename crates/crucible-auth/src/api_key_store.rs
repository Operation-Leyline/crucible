use crate::ApiKeyRecord;
use std::sync::Arc;

/// The service's key table, read by digest. Implementations own the row
/// mapping (status field, expiry format, attributes) and must not put the
/// digest into an error: errors reach the service log. A read that outlives
/// `ApiKeyAuthenticator::lookup_timeout` is dropped, so `lookup` must be safe
/// to cancel (a plain read is).
#[async_trait::async_trait]
pub trait ApiKeyStore: Send + Sync + 'static {
    /// `Ok(None)` when no key has this digest: the credential is refused.
    /// `Err` when the store cannot answer: the request fails with a sanitized
    /// 5xx and nothing is cached, never a refusal.
    async fn lookup(
        &self,
        digest: &str,
    ) -> Result<Option<ApiKeyRecord>, crucible_server_runtime_rust::Error>;
}

/// Lets a service keep a handle on the store it gives the authenticator.
#[async_trait::async_trait]
impl<T: ApiKeyStore + ?Sized> ApiKeyStore for Arc<T> {
    async fn lookup(
        &self,
        digest: &str,
    ) -> Result<Option<ApiKeyRecord>, crucible_server_runtime_rust::Error> {
        (**self).lookup(digest).await
    }
}
