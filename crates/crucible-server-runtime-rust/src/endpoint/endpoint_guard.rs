use crate::Hook;
use axum::http::request::Parts;
use axum::response::Response;

/// Endpoint authentication also covers messages that have no operation.
pub enum EndpointAdmission {
    Admit,
    Respond(Response),
}

pub trait EndpointGuard: Send + Sync + 'static {
    fn admit<'a>(&'a self, request: &'a Parts) -> Hook<'a, EndpointAdmission>;
}

impl<F> EndpointGuard for F
where
    F: Fn(&Parts) -> EndpointAdmission + Send + Sync + 'static,
{
    fn admit<'a>(&'a self, request: &'a Parts) -> Hook<'a, EndpointAdmission> {
        Hook::Ready(self(request))
    }
}
