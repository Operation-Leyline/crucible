use std::fmt;

/// The Authorizer could not tell. Its error stays the source for the
/// service log; the caller sees only the service's internal error, since an
/// outage is never a reason to allow a request, nor to refuse it as if the
/// caller lacked the permission.
#[derive(Debug)]
pub(crate) struct AuthorizerUnavailable(pub crate::Error);

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
