use std::fmt;

/// A [`BindingSource`](super::BindingSource) could not answer: its store is
/// unreachable, or timed out. `RoleAuthorizer` then answers the batch as an
/// `Authorizer` error, which `Permissions` turns into the service's
/// sanitized internal error: never an allow, and never a refusal that reads
/// as a missing permission.
#[derive(Debug)]
pub struct Unavailable(Box<dyn std::error::Error + Send + Sync>);

impl Unavailable {
    /// `cause` stays the source, for the service log.
    pub fn new(cause: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self(cause.into())
    }
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the role bindings could not be read")
    }
}

impl std::error::Error for Unavailable {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.0)
    }
}
