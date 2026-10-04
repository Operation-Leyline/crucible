//! `crucible.protocols#json` (smithy/crucible.smithy): members bound to the
//! URI path, query string and headers, the rest in a JSON object body;
//! errors carry `"__type"`.

use axum::body::Body;
use axum::http::request::Parts;
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::borrow::Cow;

use serde_json::Value;

use super::text::{percent_decode, query_values, split_header};
use super::{Answers, BoundValue, Invocation, Protocol, Route};
use crate::error::{FrameworkError, ModelledError, Result};
use crate::handler::{Handler, Hook, Job, Resolution};
use crate::operation::{Location, MemberInfo, OperationInfo, Scalar, Segment};
use crate::operations::Operations;
use crate::time;
use crate::validate::{Rejection, Violation, ViolationKind};

#[derive(Debug, Clone, Copy, Default)]
pub struct JsonProtocol;

impl Protocol for JsonProtocol {
    fn route(&self, request: &Parts, operations: &Operations) -> Option<Route> {
        let path: Vec<&str> = request
            .uri
            .path()
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let mut best: Option<(usize, Route)> = None;
        for &op in operations.routes(path.len()) {
            if op.method != request.method.as_str() {
                continue;
            }
            let mut labels = Vec::new();
            let mut literals = 0;
            let matched = op
                .path
                .iter()
                .zip(&path)
                .all(|(segment, part)| match segment {
                    Segment::Literal(text) => {
                        literals += 1;
                        text == part
                    }
                    Segment::Label(name) => {
                        labels.push((*name, percent_decode(part, false).into_owned()));
                        true
                    }
                });
            // A literal beats a label in the same place: `cities/search`
            // over `cities/{cityId}`.
            if matched && best.as_ref().is_none_or(|(most, _)| literals > *most) {
                best = Some((
                    literals,
                    Route {
                        operation: op.name,
                        labels,
                    },
                ));
            }
        }
        best.map(|(_, route)| route)
    }

    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response {
        let operation = job.operation();
        match job.resolution() {
            Some(Resolution::Success(output)) => {
                let op = operation.expect("an output implies an operation");
                let has_body = op.output.iter().any(|m| m.location == Location::Body);
                let has_headers = op
                    .output
                    .iter()
                    .any(|m| matches!(m.location, Location::Header(_)));
                if !has_headers {
                    // Straight to bytes, with no JSON value in between.
                    let mut body = Vec::with_capacity(256);
                    let written = !has_body || output.write_json(&mut body).is_ok();
                    return respond(op.status, Vec::new(), (has_body && written).then_some(body));
                }
                // Header members leave the body as it is written.
                let mut body = Vec::with_capacity(256);
                match output.write_json_split(op.output, &mut body) {
                    Ok(headers) => respond(op.status, headers, has_body.then_some(body)),
                    Err(_) => framework_response(&FrameworkError::InternalFailure),
                }
            }
            Some(Resolution::Modelled(error)) => error_response(error),
            Some(Resolution::Rejected(rejection)) => error_response(&*(answers.reject)(rejection)),
            Some(Resolution::Framework(error)) => framework_response(&error),
            Some(Resolution::Internal(error)) => {
                tracing::error!(
                    request_id = job.context().request_id(),
                    operation = operation.map_or("unknown", |op| op.name),
                    error = error.chain(),
                    "internal failure"
                );
                match answers.internal {
                    Some(internal) => error_response(&*internal()),
                    None => framework_response(&FrameworkError::InternalFailure),
                }
            }
            None => framework_response(&FrameworkError::InternalFailure),
        }
    }
}

impl Handler for JsonProtocol {
    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        Hook::Ready(read_input(job))
    }

    fn as_protocol(&self) -> Option<&dyn Protocol> {
        Some(self)
    }
}

/// Reads the input: bound members from the request's labels, query and
/// headers, the rest from the JSON body, then validates it.
fn read_input(job: &mut Job) -> Result<()> {
    // A direct invocation is the invoke protocol's to read.
    if job.request().extensions.get::<Invocation>().is_some() || job.input().is_some() {
        return Ok(());
    }
    let Some(op) = job.operation() else {
        return Err(FrameworkError::UnknownOperation.into());
    };
    // Either way the input is read in one pass, with no JSON value in
    // between: from the body alone, or from the bound members and the
    // body together.
    let input = if op.input.iter().all(|m| m.location == Location::Body) {
        job.decode_json(job.body())?
    } else {
        let bound = bound_members(op, job)?;
        job.decode_bound(job.body(), bound)?
    };
    job.set_input(input);
    Ok(())
}

/// The bound members, checked against their kinds. The body is read with
/// them, in one pass, by `Job::decode_bound`.
fn bound_members<'a>(
    op: &OperationInfo,
    job: &'a Job,
) -> std::result::Result<Vec<(&'static str, BoundValue<'a>)>, Rejection> {
    let query = job.request().uri.query().unwrap_or("");
    let mut bound = Vec::new();
    let mut violations = Vec::new();
    for member in op.input {
        let wire = wire_name(member);
        let value = match (member.location, member.scalar) {
            (Location::Body, _) => continue,
            (Location::Label, scalar) => {
                match job.labels().iter().find(|(name, _)| *name == member.name) {
                    Some((_, text)) => from_text(Cow::Borrowed(text.as_str()), scalar),
                    None => continue,
                }
            }
            (Location::Query(param), Scalar::List(item)) => {
                let items: std::result::Result<Vec<BoundValue<'a>>, String> =
                    query_values(query, param)
                        .map(|text| from_text(text, *item))
                        .collect();
                match items {
                    Ok(items) if items.is_empty() => continue,
                    other => other.map(BoundValue::List),
                }
            }
            (Location::Query(param), scalar) => match query_values(query, param).next() {
                Some(text) => from_text(text, scalar),
                None => continue,
            },
            (Location::Header(name), scalar) => {
                let Some(value) = job.request().headers.get(name) else {
                    continue;
                };
                let Ok(text) = value.to_str() else {
                    violations.push(malformed(name, "is not visible ASCII"));
                    continue;
                };
                match scalar {
                    Scalar::List(item) => split_header(text)
                        .into_iter()
                        .map(|text| from_text(Cow::Owned(text), *item))
                        .collect::<std::result::Result<Vec<_>, _>>()
                        .map(BoundValue::List),
                    scalar => from_text(Cow::Borrowed(text), scalar),
                }
            }
        };
        match value {
            Ok(value) => bound.push((member.name, value)),
            Err(message) => violations.push(malformed(wire, &message)),
        }
    }
    if violations.is_empty() {
        Ok(bound)
    } else {
        Err(Rejection { violations })
    }
}

/// The name a bound member goes by on the wire, for a violation's path.
fn wire_name(member: &MemberInfo) -> &'static str {
    match member.location {
        Location::Query(name) | Location::Header(name) => name,
        Location::Label | Location::Body => member.name,
    }
}

fn malformed(path: &str, message: &str) -> Violation {
    Violation {
        path: path.to_owned(),
        message: message.to_owned(),
        kind: ViolationKind::Malformed,
    }
}

/// A bound member's text as its kind, or why it is not one.
fn from_text(text: Cow<'_, str>, scalar: Scalar) -> std::result::Result<BoundValue<'_>, String> {
    match scalar {
        Scalar::String | Scalar::Json => Ok(BoundValue::Text(text)),
        Scalar::Integer => text
            .parse::<i64>()
            .map(BoundValue::Integer)
            .map_err(|_| format!("must be an integer (`{text}` is not)")),
        Scalar::Float | Scalar::EpochSeconds => text
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .map(BoundValue::Float)
            .ok_or_else(|| format!("must be a number (`{text}` is not)")),
        Scalar::Boolean => match &*text {
            "true" => Ok(BoundValue::Boolean(true)),
            "false" => Ok(BoundValue::Boolean(false)),
            _ => Err("must be true or false".to_owned()),
        },
        Scalar::DateTime => match time::parse_date_time(&text) {
            Some(_) => Ok(BoundValue::Text(text)),
            None => Err("must be an RFC 3339 timestamp".to_owned()),
        },
        Scalar::HttpDate => match time::http_date::parse(&text) {
            Some(_) => Ok(BoundValue::Text(text)),
            None => Err("must be an HTTP date".to_owned()),
        },
        Scalar::List(_) => Err("a list cannot hold a list here".to_owned()),
    }
}

fn error_response(error: &dyn ModelledError) -> Response {
    let mut body = Vec::with_capacity(128);
    match error.write_json(&mut body) {
        Ok(headers) => respond(error.info().status, headers, Some(body)),
        Err(_) => framework_response(&FrameworkError::InternalFailure),
    }
}

fn framework_response(error: &FrameworkError) -> Response {
    let body = serde_json::json!({ "__type": error.name(), "message": error.message() });
    respond(error.status(), Vec::new(), Some(to_bytes(&body)))
}

fn to_bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec())
}

fn respond(status: u16, headers: Vec<(&'static str, String)>, body: Option<Vec<u8>>) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = match body {
        Some(body) => (
            status,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )],
            body,
        )
            .into_response(),
        None => (status, Body::empty()).into_response(),
    };
    for (name, value) in headers {
        // Generated header names are lowercase, so they parse without copying.
        let name =
            HeaderName::from_lowercase(name.as_bytes()).or_else(|_| HeaderName::try_from(name));
        if let (Ok(name), Ok(value)) = (name, HeaderValue::try_from(value)) {
            response.headers_mut().insert(name, value);
        }
    }
    response
}
