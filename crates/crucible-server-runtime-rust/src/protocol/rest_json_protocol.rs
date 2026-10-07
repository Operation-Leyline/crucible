use axum::http::request::Parts;
use axum::response::Response;

use super::{Answers, JsonProtocol, Protocol, Route};
use crate::operation::OperationInfo;
use crate::{Hook, Job, Operations, Result};

/// AWS REST JSON with modeled HTTP bindings and `X-Amzn-Errortype` errors.
#[derive(Debug, Clone, Copy, Default)]
pub struct RestJsonProtocol;

impl Protocol for RestJsonProtocol {
    fn route(&self, request: &Parts, operations: &Operations) -> Option<Route> {
        JsonProtocol.route(request, operations)
    }
    fn route_scoped(
        &self,
        request: &Parts,
        operations: &Operations,
        applies: &dyn Fn(&OperationInfo) -> bool,
    ) -> Option<Route> {
        JsonProtocol.route_scoped(request, operations, applies)
    }
    fn decode<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        JsonProtocol.decode(job)
    }
    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response {
        super::json_response::write(job, answers, super::json_response::Format::RestJson)
    }
}
