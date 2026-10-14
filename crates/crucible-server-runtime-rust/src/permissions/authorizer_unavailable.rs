use std::fmt;

use crate::OpaqueError;

/// The Authorizer could not tell. Its error stays the source for the
/// service log; the caller sees only the service's internal error, since an
/// outage is never a reason to allow a request, nor to refuse it as if the
/// caller lacked the permission.
#[derive(Debug)]
pub(crate) struct AuthorizerUnavailable(crate::Error);

impl AuthorizerUnavailable {
    /// The request's failure for an Authorizer error. Opaque: a service rule
    /// for its own client's errors must not answer an outage as a modelled
    /// 403 or 404 when the Authorizer uses the same client.
    pub(crate) fn error(cause: crate::Error) -> crate::Error {
        crate::Error::new(OpaqueError::new(Self(cause)))
    }
}

impl fmt::Display for AuthorizerUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the Authorizer could not decide")
    }
}

impl std::error::Error for AuthorizerUnavailable {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.get_ref())
    }
}
