use crucible_server_runtime_rust::Error;
use std::fmt;

/// Preserve the provider cause while ensuring it cannot be classified as a client refusal.
#[derive(Debug)]
pub(crate) struct AuthenticationUnavailable(pub Error);

impl fmt::Display for AuthenticationUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("authentication provider unavailable")
    }
}

impl std::error::Error for AuthenticationUnavailable {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.get_ref())
    }
}
