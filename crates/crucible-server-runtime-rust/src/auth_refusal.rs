use std::fmt;

use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

/// A framework authentication/authorization refusal, independent of modeled errors.
/// Return it from middleware with `?`; every protocol preserves its HTTP status
/// and challenge. Applications can also supply a response with `Job::respond`.
#[derive(Debug, Clone)]
pub struct AuthRefusal {
    status: StatusCode,
    headers: HeaderMap,
}

impl AuthRefusal {
    pub fn unauthorized(headers: HeaderMap) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            headers,
        }
    }
    pub fn forbidden(headers: HeaderMap) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            headers,
        }
    }
    pub(crate) fn response(&self) -> Response {
        (self.status, self.headers.clone()).into_response()
    }
}
impl fmt::Display for AuthRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.status)
    }
}
impl std::error::Error for AuthRefusal {}
