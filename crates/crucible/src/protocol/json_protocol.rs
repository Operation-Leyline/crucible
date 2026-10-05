//! `crucible.protocols#json` (smithy/crucible.smithy): members bound to the
//! URI path, query string and headers, the rest in a JSON object body, or a
//! blob payload as the whole body, raw; errors carry `"__type"`.

use axum::body::Body;
use axum::http::request::Parts;
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::borrow::Cow;

use serde_json::Value;

use super::{Answers, BoundValue, Invocation, Protocol, Route};
use crate::error::{FrameworkError, ModelledError, Result};
use crate::handler::{Handler, Hook, Job, Resolution};
use crate::operation::{Location, MemberInfo, OperationInfo, Scalar, Segment};
use crate::operations::Operations;
use crate::validate::{Rejection, Violation, ViolationKind};
use crucible_wire::text::{percent_decode, query_values};

#[derive(Debug, Clone, Copy, Default)]
pub struct JsonProtocol;

impl Protocol for JsonProtocol {
    fn route(&self, request: &Parts, operations: &Operations) -> Option<Route> {
        let rest = request.uri.path().strip_prefix('/')?;
        let path: Vec<&str> = if rest.is_empty() {
            Vec::new()
        } else {
            rest.split('/').collect()
        };
        let mut best: Option<((usize, usize, usize), Route)> = None;
        for &op in operations
            .routes(path.len())
            .iter()
            .chain(operations.greedy_routes())
        {
            if op.method != request.method.as_str() {
                continue;
            }
            let query = request.uri.query().unwrap_or("");
            if !op.literal_query.iter().all(|(key, expected)| {
                query_values(query, key)
                    .any(|value| expected.is_none_or(|expected| value == expected))
            }) {
                continue;
            }
            let mut labels = Vec::new();
            let mut literals = 0;
            if path.len() < op.path.len() {
                continue;
            }
            let mut cursor = 0;
            let matched = op.path.iter().all(|segment| {
                let Some(part) = path.get(cursor) else {
                    return false;
                };
                cursor += 1;
                match segment {
                    Segment::Literal(text) => {
                        literals += 1;
                        text == part
                    }
                    Segment::Label(name) => {
                        if part.is_empty() {
                            return false;
                        }
                        labels.push((*name, percent_decode(part, false).into_owned()));
                        true
                    }
                    Segment::GreedyLabel(name) => {
                        let width = path.len() - op.path.len() + 1;
                        let joined = path[cursor - 1..cursor - 1 + width].join("/");
                        cursor += width - 1;
                        if joined.is_empty() {
                            return false;
                        }
                        labels.push((*name, percent_decode(&joined, false).into_owned()));
                        true
                    }
                }
            }) && cursor == path.len();
            // A literal beats a label in the same place: `cities/search`
            // over `cities/{cityId}`. With equal query constraint counts,
            // an exact value beats a constraint that only requires the key.
            let exact_query_values = op
                .literal_query
                .iter()
                .filter(|(_, expected)| expected.is_some())
                .count();
            let specificity = (literals, op.literal_query.len(), exact_query_values);
            if matched && best.as_ref().is_none_or(|(most, _)| specificity > *most) {
                best = Some((
                    specificity,
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
                if let Some(stream) = output.stream() {
                    let mut ignored = Vec::new();
                    let bindings = match output.write_http(op.output, &mut ignored) {
                        Ok(bindings) => bindings,
                        Err(error) => return serialization_failure(error),
                    };
                    let Some((member, media_type)) = Location::payload(op.output) else {
                        return framework_response(&FrameworkError::InternalFailure);
                    };
                    let stream = match member.location {
                        Location::StreamingPayload {
                            minimum, maximum, ..
                        } => stream.with_bounds(minimum, maximum),
                        _ => stream,
                    };
                    return match stream.open() {
                        Ok(body) => {
                            let mut response = respond(
                                bindings.status.unwrap_or(op.status),
                                bindings.headers,
                                None,
                            );
                            response
                                .headers_mut()
                                .insert(header::CONTENT_TYPE, content_type_header(media_type));
                            if let Some(length) = stream.length() {
                                response.headers_mut().insert(
                                    header::CONTENT_LENGTH,
                                    HeaderValue::from_str(&length.to_string())
                                        .expect("integer header"),
                                );
                            }
                            *response.body_mut() = Body::from_stream(body);
                            response
                        }
                        Err(_) => framework_response(&FrameworkError::InternalFailure),
                    };
                }
                if let Some((_, media_type)) = Location::payload(op.output) {
                    // The payload's bytes are the body, under its own media type.
                    let mut body = Vec::new();
                    return match output.write_http(op.output, &mut body) {
                        Ok(bindings) => respond(
                            bindings.status.unwrap_or(op.status),
                            bindings.headers,
                            (!body.is_empty()).then_some((media_type, body)),
                        ),
                        Err(error) => serialization_failure(error),
                    };
                }
                let has_body = op.output.iter().any(|m| m.location == Location::Body);
                let has_headers = op.output.iter().any(|m| {
                    matches!(
                        m.location,
                        Location::Header(_) | Location::PrefixHeaders(_) | Location::ResponseCode
                    )
                });
                if !has_headers {
                    // Straight to bytes, with no JSON value in between.
                    if !has_body {
                        return respond(op.status, Vec::new(), None);
                    }
                    let mut body = Vec::with_capacity(256);
                    return match output.write_json(&mut body) {
                        Ok(()) => respond(op.status, Vec::new(), Some((JSON, body))),
                        Err(error) => serialization_failure(error),
                    };
                }
                // Header members leave the body as it is written.
                let mut body = Vec::with_capacity(256);
                match output.write_http(op.output, &mut body) {
                    Ok(bindings) => respond(
                        bindings.status.unwrap_or(op.status),
                        bindings.headers,
                        has_body.then_some((JSON, body)),
                    ),
                    Err(error) => serialization_failure(error),
                }
            }
            Some(Resolution::Modelled(error)) => error_response(error),
            Some(Resolution::Rejected(rejection)) => error_response(&*(answers.reject)(rejection)),
            Some(Resolution::Framework(FrameworkError::UnknownOperation))
                if answers.unknown.is_some() =>
            {
                error_response(&*(answers.unknown.expect("checked"))())
            }
            Some(Resolution::Framework(error)) => framework_response(&error),
            Some(Resolution::Internal(_)) => match answers.internal {
                Some(internal) => error_response(&*internal()),
                None => framework_response(&FrameworkError::InternalFailure),
            },
            // An answered request never reaches the protocol.
            Some(Resolution::Answered { .. }) | None => {
                framework_response(&FrameworkError::InternalFailure)
            }
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
/// headers, the rest from the JSON body, then validates it. A payload is the
/// body itself, unparsed, whatever its content type.
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
        // A payload took the body as bytes: there is no JSON to read.
        let body = match Location::payload(op.input) {
            Some(_) => &[][..],
            None => job.body(),
        };
        job.decode_bound(body, bound)?
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
            (Location::Body | Location::ResponseCode, _) => continue,
            (Location::QueryParams, scalar) => {
                let mut values = serde_json::Map::new();
                for pair in query.split('&').filter(|p| !p.is_empty()) {
                    let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
                    let name = percent_decode(name, true).into_owned();
                    if op.literal_query.iter().any(|(key, _)| *key == name)
                        || op
                            .input
                            .iter()
                            .any(|m| matches!(m.location, Location::Query(key) if key == name))
                    {
                        continue;
                    }
                    let value = Value::String(percent_decode(value, true).into_owned());
                    if matches!(scalar, Scalar::List(_)) {
                        values
                            .entry(name)
                            .or_insert_with(|| Value::Array(Vec::new()))
                            .as_array_mut()
                            .expect("list map")
                            .push(value);
                    } else {
                        values.entry(name).or_insert(value);
                    }
                }
                Ok(BoundValue::Json(Value::Object(values)))
            }
            (Location::PrefixHeaders(prefix), _) => {
                let mut values = serde_json::Map::new();
                for (name, value) in &job.request().headers {
                    if let Some(key) = name.as_str().strip_prefix(prefix) {
                        match value.to_str() {
                            Ok(text) => {
                                values.insert(key.to_owned(), Value::String(text.to_owned()));
                            }
                            Err(_) => {
                                violations.push(malformed(name.as_str(), "is not visible ASCII"))
                            }
                        }
                    }
                }
                Ok(BoundValue::Json(Value::Object(values)))
            }
            (
                Location::StreamingPayload {
                    requires_length, ..
                },
                _,
            ) => {
                if requires_length && job.request().headers.get("content-length").is_none() {
                    violations.push(malformed(
                        member.name,
                        "streaming payload requires content-length",
                    ));
                    continue;
                }
                Ok(BoundValue::Json(Value::Null))
            }
            // An empty body is no payload: a required one is missing.
            (Location::Payload(_), _) if job.body().is_empty() => continue,
            (Location::Payload(_), _) => Ok(BoundValue::Bytes(job.body().clone())),
            (Location::TextPayload(_), _) => std::str::from_utf8(job.body())
                .map(|text| BoundValue::Text(Cow::Borrowed(text)))
                .map_err(|_| "must be UTF-8 text".to_owned()),
            (Location::DocumentPayload(_), _) if job.body().trim_ascii().is_empty() => continue,
            (Location::DocumentPayload(_), _) => Ok(BoundValue::RawJson(job.body())),
            (Location::Label, scalar) => {
                match job.labels().iter().find(|(name, _)| *name == member.name) {
                    Some((_, text)) => BoundValue::parse(Cow::Borrowed(text.as_str()), scalar),
                    None => continue,
                }
            }
            (Location::Query(param), Scalar::List(item)) => {
                let items: std::result::Result<Vec<BoundValue<'a>>, String> =
                    query_values(query, param)
                        .map(|text| BoundValue::parse(text, *item))
                        .collect();
                match items {
                    Ok(items) if items.is_empty() => continue,
                    other => other.map(BoundValue::List),
                }
            }
            (Location::Query(param), scalar) => match query_values(query, param).next() {
                Some(text) => BoundValue::parse(text, scalar),
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
                BoundValue::parse(Cow::Borrowed(text), scalar)
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
        Location::Label
        | Location::Body
        | Location::Payload(_)
        | Location::TextPayload(_)
        | Location::DocumentPayload(_)
        | Location::QueryParams
        | Location::PrefixHeaders(_)
        | Location::ResponseCode
        | Location::StreamingPayload { .. } => member.name,
    }
}

fn malformed(path: &str, message: &str) -> Violation {
    Violation {
        path: path.to_owned(),
        message: message.to_owned(),
        kind: ViolationKind::Malformed,
    }
}

const JSON: &str = "application/json";

fn error_response(error: &dyn ModelledError) -> Response {
    let mut body = Vec::with_capacity(128);
    match error.write_http(&mut body) {
        Ok(bindings) => respond(
            bindings.status.unwrap_or(error.info().status),
            bindings.headers,
            Some((JSON, body)),
        ),
        Err(error) => serialization_failure(error),
    }
}

fn framework_response(error: &FrameworkError) -> Response {
    let body = serde_json::json!({ "__type": error.name(), "message": error.message() });
    respond(error.status(), Vec::new(), Some((JSON, to_bytes(&body))))
}

fn serialization_failure(error: serde_json::Error) -> Response {
    let mut response = framework_response(&FrameworkError::InternalFailure);
    response
        .extensions_mut()
        .insert(super::SerializationFailure(error.to_string()));
    response
}

fn to_bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec())
}

/// A JSON body's content type costs no allocation; a payload's media type
/// is checked by lowering, so the fallback is for hand-written descriptions.
fn content_type_header(content_type: &'static str) -> HeaderValue {
    if content_type == JSON {
        HeaderValue::from_static(JSON)
    } else {
        HeaderValue::from_str(content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream"))
    }
}

/// `body` is the content type and the bytes.
fn respond(
    status: u16,
    headers: Vec<(String, String)>,
    body: Option<(&'static str, Vec<u8>)>,
) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = match body {
        Some((content_type, body)) => (
            status,
            [(header::CONTENT_TYPE, content_type_header(content_type))],
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
