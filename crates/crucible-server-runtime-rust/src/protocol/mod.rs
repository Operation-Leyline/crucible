//! Protocols identify an operation, decode its input, and encode its outcome.
//! Several can serve one service; the first to claim a request serves it.

mod invoke_protocol;
mod json_protocol;
mod json_response;
mod server_events;

use axum::http::request::Parts;
use axum::response::Response;

pub use crucible_wire::bound::BoundValue;
pub use invoke_protocol::{Invocation, InvokeProtocol};
pub use json_protocol::JsonProtocol;

use crate::error::ModelledError;
use crate::operation::OperationInfo;
use crate::operations::Operations;
use crate::validate::Rejection;
use crate::{Hook, Job, Prepare, Result};

/// Whether the request's `content-type` is `expected`, ignoring case and
/// parameters.
pub(super) fn content_type(request: &Parts, expected: &str) -> bool {
    let mut values = request.headers.get_all("content-type").iter();
    // Exactly one field: a repeated `Content-Type` is ambiguous, not a match.
    let (Some(value), None) = (values.next(), values.next()) else {
        return false;
    };
    value.to_str().ok().is_some_and(|v| {
        v.split(';')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case(expected)
    })
}

/// Carries serialization diagnostics to completion observers, never the wire.
#[derive(Debug, Clone)]
pub(crate) struct SerializationFailure(pub String);

/// Which operation a request is, and the URI labels it carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub operation: &'static str,
    /// Label names (from the model) and their decoded values.
    pub labels: Vec<(&'static str, String)>,
    /// A malformed URI still claims its route, so it cannot fall through to
    /// another operation or become an unknown-operation response.
    pub rejection: Option<Rejection>,
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

/// Whether a protocol serves this request. A claim fixes the protocol even
/// when its envelope is malformed or no operation matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolMatch {
    Declined,
    /// The request belongs to this protocol. `None` means its operation is
    /// unknown; return an error for a malformed envelope instead.
    Claimed(Option<Route>),
}

pub trait Protocol: Send + Sync + 'static {
    /// Whether this protocol can encode failures outside its owned requests.
    /// Dedicated endpoints opt out so they do not replace ordinary API errors.
    #[doc(hidden)]
    fn fallback(&self) -> bool {
        true
    }

    /// Whether this protocol serves `request` at the current API version
    /// only, whatever it names: its callers are built from the current
    /// model, such as MCP tools and direct invocations (docs/api-versions.md).
    #[doc(hidden)]
    fn current_version_only(&self, request: &Parts) -> bool {
        let _ = request;
        false
    }

    /// Endpoint admission before transport middleware or body parsing. Only the
    /// protocol owning the request runs this hook; `Job::respond` stops dispatch.
    #[doc(hidden)]
    fn admit<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        let _ = job;
        Hook::Ready(Ok(()))
    }

    /// Identifies the operation before operation-aware middleware runs.
    /// Envelope-based protocols may read a bounded body here and retain
    /// decoded envelope state for `decode`; declining must leave the request
    /// available to later protocols.
    fn identify<'a>(
        &'a self,
        job: &'a mut Job,
        operations: &'a Operations,
    ) -> Hook<'a, Result<ProtocolMatch>> {
        self.identify_scoped(job, operations, &|_| true)
    }

    /// Identifies among accepted operations. Envelope-based protocols should
    /// override this hook so operation scopes apply to asynchronous routing
    /// too; `identify` delegates here with an unrestricted scope.
    fn identify_scoped<'a>(
        &'a self,
        job: &'a mut Job,
        operations: &'a Operations,
        applies: &'a (dyn Fn(&OperationInfo) -> bool + Send + Sync),
    ) -> Hook<'a, Result<ProtocolMatch>> {
        let matched = match self.route_scoped(job.request(), operations, applies) {
            Some(route) => ProtocolMatch::Claimed(Some(route)),
            // A known operation excluded by scope belongs to later protocols.
            None if self.route(job.request(), operations).is_some() => ProtocolMatch::Declined,
            None if self.owns(job.request()) => ProtocolMatch::Claimed(None),
            None => ProtocolMatch::Declined,
        };
        Hook::Ready(Ok(matched))
    }

    /// Header/URI routing for protocols that need no envelope parsing.
    /// An asynchronous-only protocol implements `identify_scoped` instead.
    fn route(&self, request: &Parts, operations: &Operations) -> Option<Route> {
        let _ = (request, operations);
        None
    }

    /// Reads and validates the selected operation's input. The orchestrator
    /// has prepared its bounded body before this hook runs.
    fn decode<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>>;

    /// Reads model metadata once at service construction.
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        let _ = prepare;
        Ok(())
    }

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

    /// Claims requests in this format even when no operation matches.
    /// A claim prevents later protocols from interpreting the same request.
    fn owns(&self, request: &Parts) -> bool {
        let _ = request;
        false
    }

    /// Encodes the outcome after interceptors and operation-aware middleware
    /// unwind, before transport middleware transforms the response.
    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response;

    /// Final protocol headers, including directly answered admission refusals.
    #[doc(hidden)]
    fn decorate_response(&self, response: &mut Response) {
        let _ = response;
    }
}

#[cfg(test)]
mod tests {
    use axum::http::Request;

    #[test]
    fn media_type_matching_ignores_case_and_parameters_but_not_other_types() {
        for (value, matches) in [
            ("application/json", true),
            ("Application/JSON; charset=utf-8", true),
            (
                " application/json ; charset=\"UTF-8\"; custom=parameter",
                true,
            ),
            ("application/hal+json", false),
            ("application/json, application/json", false),
            ("text/plain", false),
        ] {
            let (parts, _) = Request::post("/")
                .header("content-type", value)
                .body(())
                .unwrap()
                .into_parts();
            assert_eq!(
                super::content_type(&parts, "application/json"),
                matches,
                "{value}"
            );
        }
        let (parts, _) = Request::post("/").body(()).unwrap().into_parts();
        assert!(!super::content_type(&parts, "application/json"));
    }
}
