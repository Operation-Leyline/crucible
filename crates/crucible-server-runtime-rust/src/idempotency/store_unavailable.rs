use std::fmt;

/// A store failure. The store's error stays the source for the service log;
/// the caller sees only the service's internal error, since an outage is
/// never a reason to run an operation twice or to refuse a request as a
/// duplicate.
#[derive(Debug)]
pub(crate) struct StoreUnavailable(pub crate::Error);

impl fmt::Display for StoreUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("idempotency store unavailable")
    }
}

impl std::error::Error for StoreUnavailable {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.get_ref())
    }
}
