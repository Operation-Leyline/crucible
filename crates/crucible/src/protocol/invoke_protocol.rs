//! Direct invocation: an operation called by name with its whole input as a
//! JSON object, no HTTP bindings. Lambda's direct invokes arrive this way
//! (`host::lambda`), and so can any in-process caller (`Crucible::invoke`).
//! The answer is `{"output": ...}` or `{"error": {"__type": ..., ...}}`.
//! Operations with streaming payloads use HTTP; direct invocation rejects
//! them before the implementation runs, rather than serializing streams as null.

use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use super::{Answers, Protocol, Route};
use crate::error::{FrameworkError, ModelledError, Result};
use crate::handler::{Handler, Hook, Job, Resolution};
use crate::operation::Location;
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
            rejection: None,
        })
    }

    fn owns(&self, request: &Parts) -> bool {
        request.extensions.get::<Invocation>().is_some()
    }

    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response {
        let (status, body) = match job.resolution() {
            Some(Resolution::Success(output)) => {
                // `{"output": ..}` written straight to bytes.
                let mut body = Vec::with_capacity(256);
                body.extend_from_slice(b"{\"output\":");
                match output.write_json(&mut body) {
                    Ok(()) => {
                        body.push(b'}');
                        return respond(200, body);
                    }
                    Err(error) => {
                        let (status, body) = framework_body(&FrameworkError::InternalFailure);
                        let mut response =
                            respond(status, serde_json::to_vec(&body).expect("failure envelope"));
                        response
                            .extensions_mut()
                            .insert(super::SerializationFailure(error.to_string()));
                        return response;
                    }
                }
            }
            Some(Resolution::Modelled(error)) => error_body(error),
            Some(Resolution::Rejected(rejection)) => error_body(&*(answers.reject)(rejection)),
            Some(Resolution::Framework(FrameworkError::UnknownOperation))
                if answers.unknown.is_some() =>
            {
                error_body(&*(answers.unknown.expect("checked"))())
            }
            Some(Resolution::Framework(error)) => framework_body(&error),
            Some(Resolution::Internal(_)) => match answers.internal {
                Some(internal) => error_body(&*internal()),
                None => framework_body(&FrameworkError::InternalFailure),
            },
            // An answered request never reaches the protocol.
            Some(Resolution::Answered { .. }) | None => {
                framework_body(&FrameworkError::InternalFailure)
            }
        };
        respond(status, serde_json::to_vec(&body).unwrap_or_default())
    }
}

fn respond(status: u16, body: Vec<u8>) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )],
        body,
    )
        .into_response()
}

impl Handler for InvokeProtocol {
    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        Hook::Ready(self.read_input(job))
    }

    fn as_protocol(&self) -> Option<&dyn Protocol> {
        Some(self)
    }
}

impl InvokeProtocol {
    /// The whole input is the request body.
    fn read_input(&self, job: &mut Job) -> Result<()> {
        if !self.owns(job.request()) {
            return Ok(());
        }
        let operation = job.operation().ok_or(FrameworkError::UnknownOperation)?;
        if operation
            .input
            .iter()
            .chain(operation.output)
            .any(|member| matches!(member.location, Location::StreamingPayload { .. }))
        {
            return Err(Rejection::malformed(
                "",
                "direct invocation does not support streaming payloads; use HTTP",
            )
            .into());
        }
        let input = job.decode_json(job.body())?;
        job.set_input(input);
        Ok(())
    }
}

fn error_body(error: &dyn ModelledError) -> (u16, Value) {
    // Header members stay in an invoke's error: there are no headers.
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
