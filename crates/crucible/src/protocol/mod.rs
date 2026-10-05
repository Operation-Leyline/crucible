//! Protocols: handlers that also route a request to an operation and write
//! the response. Several can serve one service (an RPC and a REST one, say);
//! the first in the chain to claim a request serves it.

mod invoke_protocol;
mod json_protocol;

use axum::http::request::Parts;
use axum::response::Response;

pub use crucible_wire::bound::BoundValue;
pub use invoke_protocol::{Invocation, InvokeProtocol};
pub use json_protocol::JsonProtocol;

use crate::error::ModelledError;
use crate::handler::Job;
use crate::operation::OperationInfo;
use crate::operations::Operations;
use crate::validate::Rejection;

/// Carries serialization diagnostics to completion observers, never the wire.
#[derive(Debug, Clone)]
pub(crate) struct SerializationFailure(pub String);

/// Which operation a request is, and the URI labels it carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub operation: &'static str,
    /// Label names (from the model) and their decoded values.
    pub labels: Vec<(&'static str, String)>,
}

/// Builds the response errors are answered with.
pub struct Answers<'a> {
    /// The validation error a rejection becomes.
    pub reject: &'a (dyn Fn(&Rejection) -> Box<dyn ModelledError> + Send + Sync),
    /// The internal error, when the service models one.
    pub internal: Option<crate::operation::InternalFn>,
    /// The error for a request no operation serves, when the service models
    /// one.
    pub unknown: Option<crate::operation::InternalFn>,
}

pub trait Protocol: Send + Sync {
    // The orchestrator runs Handler hooks only for the selected protocol;
    // its body is read immediately before that handler's before hook.
    /// The operation this request is, if this protocol serves it.
    fn route(&self, request: &Parts, operations: &Operations) -> Option<Route>;

    /// Routes among operations accepted by the scope. The default filters
    /// the chosen route; protocols with overlapping candidates should override
    /// this to filter candidates before choosing the most specific route.
    fn route_scoped(
        &self,
        request: &Parts,
        operations: &Operations,
        applies: &dyn Fn(&OperationInfo) -> bool,
    ) -> Option<Route> {
        let route = self.route(request, operations)?;
        operations
            .service()
            .operation(route.operation)
            .filter(|operation| applies(operation))?;
        Some(route)
    }

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
