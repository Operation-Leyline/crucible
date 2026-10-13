use std::{fmt, sync::Arc};

/// A store failure shared by every request that joined one read. The store's
/// error stays the source for the service log; the message names no key.
#[derive(Debug)]
pub(crate) struct ApiKeyStoreUnavailable(pub Arc<crucible_server_runtime_rust::Error>);

impl fmt::Display for ApiKeyStoreUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("API key store unavailable")
    }
}

impl std::error::Error for ApiKeyStoreUnavailable {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.get_ref())
    }
}
