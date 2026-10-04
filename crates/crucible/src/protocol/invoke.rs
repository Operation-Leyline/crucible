//! Direct invocation: an operation called by name with its whole input as a
//! JSON object, no HTTP bindings. Lambda's direct invokes arrive this way
//! (`host::lambda`), and so can any in-process caller (`Crucible::invoke`).
//! The answer is `{"output": ...}` or `{"error": {"__type": ..., ...}}`.

use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use super::{Answers, Protocol, Route};
use crate::error::{FrameworkError, ModelledError, Result};
use crate::handler::{Handler, Job, Resolution};
use crate::operations::Operations;
use crate::validate::Rejection;

/// Marks a request as a direct invocation of `operation`. Set only by the
/// framework, as a request extension, so no HTTP client can forge one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub operation: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InvokeProtocol;

impl Protocol for InvokeProtocol {
    fn route(&self, request: &Parts, operations: &Operations) -> Option<Route> {
        let invocation = request.extensions.get::<Invocation>()?;
        let entry = operations.entry(&invocation.operation)?;
        Some(Route {
            operation: entry.info.name,
            labels: Vec::new(),
        })
    }

    fn owns(&self, request: &Parts) -> bool {
        request.extensions.get::<Invocation>().is_some()
    }

    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response {
        let (status, body) = match job.resolution() {
            Some(Resolution::Success(output)) => (200, json!({ "output": output.to_json() })),
            Some(Resolution::Modelled(error)) => error_body(error),
            Some(Resolution::Rejected(rejection)) => error_body(&*(answers.reject)(rejection)),
            Some(Resolution::Framework(error)) => framework_body(&error),
            Some(Resolution::Internal(error)) => {
                tracing::error!(
                    request_id = job.context().request_id(),
                    operation = job.operation().map_or("unknown", |op| op.name),
                    error = error.chain(),
                    "internal failure"
                );
                match answers.internal {
                    Some(internal) => error_body(&*internal()),
                    None => framework_body(&FrameworkError::InternalFailure),
                }
            }
            None => framework_body(&FrameworkError::InternalFailure),
        };
        let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (
            status,
            [(header::CONTENT_TYPE, "application/json")],
            body.to_string(),
        )
            .into_response()
    }
}

#[async_trait::async_trait]
impl Handler for InvokeProtocol {
    /// The whole input is the request body.
    async fn before(&self, job: &mut Job) -> Result<()> {
        if !self.owns(job.request()) {
            return Ok(());
        }
        if job.operation().is_none() {
            return Err(FrameworkError::UnknownOperation.into());
        }
        let value = if job.body().is_empty() {
            json!({})
        } else {
            serde_json::from_slice(job.body()).map_err(|error| {
                Rejection::malformed("", format!("the input is not valid JSON: {error}"))
            })?
        };
        let input = job.decode(value)?;
        job.set_input(input);
        Ok(())
    }

    fn as_protocol(&self) -> Option<&dyn Protocol> {
        Some(self)
    }
}

fn error_body(error: &dyn ModelledError) -> (u16, Value) {
    let mut fields = error.to_json();
    if let Value::Object(object) = &mut fields {
        object.insert(
            "__type".to_owned(),
            Value::String(error.info().name.to_owned()),
        );
    }
    (error.info().status, json!({ "error": fields }))
}

fn framework_body(error: &FrameworkError) -> (u16, Value) {
    (
        error.status(),
        json!({ "error": { "__type": error.name(), "message": error.message() } }),
    )
}
