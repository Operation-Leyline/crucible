//! A failure whose causes exception translation must not look into.

use std::fmt;

/// A failure the [`ExceptionTranslator`](crate::ExceptionTranslator) does not
/// look inside. Its rules may match the `OpaqueError` itself (to answer every
/// opaque failure with a modelled 503, say), but the translator never walks
/// into what it wraps, so a rule written for the service's own failures
/// cannot match a cause that happens to share a type with them. Unmatched,
/// it answers as the service's internal error.
///
/// An extension wraps a failure in it when the failure's meaning is fixed
/// whatever its cause: an authentication provider's outage is a server
/// failure even when the provider is the same database client whose errors
/// the service translates into 4xx answers for its operations.
///
/// It is transparent everywhere else: its message is the wrapped error's,
/// and its sources are the wrapped error's sources, so the service log reads
/// as if it were not there.
pub struct OpaqueError {
    inner: Box<dyn std::error::Error + Send + Sync + 'static>,
}

impl OpaqueError {
    /// Wraps `error`; its message and sources stay as they are in the log.
    pub fn new(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            inner: Box::new(error),
        }
    }
}

impl fmt::Debug for OpaqueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl fmt::Display for OpaqueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.inner, f)
    }
}

impl std::error::Error for OpaqueError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner.source()
    }
}
