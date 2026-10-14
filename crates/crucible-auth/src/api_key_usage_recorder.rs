use crate::ApiKeyRecord;

/// Receives an authenticated key's use, at most once per key per
/// `ApiKeyAuthenticator::last_used_resolution`. It is awaited before the
/// request continues, for at most `ApiKeyAuthenticator::last_used_timeout`,
/// and cannot fail it: report a failed write yourself. A call past the
/// timeout is dropped, so it must be safe to cancel. The throttle is marked
/// before the call, so a failure is not retried within the window.
#[async_trait::async_trait]
pub trait ApiKeyUsageRecorder: Send + Sync + 'static {
    async fn record_use(&self, digest: &str, record: &ApiKeyRecord);
}
