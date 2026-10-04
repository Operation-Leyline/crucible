//! Protocols: handlers that also route a request to an operation and write
//! the response. Several can serve one service (an RPC and a REST one, say);
//! the first in the chain to claim a request serves it.

mod invoke;
mod json;
pub(crate) mod text;

use axum::http::request::Parts;
use axum::response::Response;

pub use invoke::{Invocation, InvokeProtocol};
pub use json::JsonProtocol;

use crate::error::ModelledError;
use crate::handler::Job;
use crate::operations::Operations;
use crate::validate::Rejection;

/// Which operation a request is, and the URI labels it carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub operation: &'static str,
    pub labels: Vec<(String, String)>,
}

/// Builds the response errors are answered with.
pub struct Answers<'a> {
    /// The validation error a rejection becomes.
    pub reject: &'a (dyn Fn(&Rejection) -> Box<dyn ModelledError> + Send + Sync),
    /// The internal error, when the service models one.
    pub internal: Option<crate::operation::InternalFn>,
}

pub trait Protocol: Send + Sync {
    /// The operation this request is, if this protocol serves it.
    fn route(&self, request: &Parts, operations: &Operations) -> Option<Route>;

    /// Whether this protocol answers a request no operation matched, because
    /// the request is in its format. The first protocol in the chain answers
    /// the rest.
    fn owns(&self, request: &Parts) -> bool {
        let _ = request;
        false
    }

    /// The response for the job's outcome. Called once, after every handler.
    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response;
}
