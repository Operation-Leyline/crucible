//! `crucible.protocols#json` (smithy/crucible.smithy): members bound to the
//! URI path, query string and headers, the rest in a JSON object body;
//! errors carry `"__type"`.

use axum::body::Body;
use axum::http::request::Parts;
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Number, Value};

use super::text::{join_header, parse_query, percent_decode, split_header};
use super::{Answers, Invocation, Protocol, Route};
use crate::error::{FrameworkError, ModelledError, Result};
use crate::handler::{Handler, Job, Resolution};
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
        for entry in operations.entries.values() {
            let op = entry.info;
            if op.method != request.method.as_str() || op.path.len() != path.len() {
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
                        labels.push(((*name).to_owned(), percent_decode(part, false)));
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
                let body = output.to_json();
                let (headers, body) = split_members(op.output, body);
                let body = op
                    .output
                    .iter()
                    .any(|m| m.location == Location::Body)
                    .then_some(body);
                respond(op.status, headers, body)
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

#[async_trait::async_trait]
impl Handler for JsonProtocol {
    /// Reads the input: bound members from the request's labels, query and
    /// headers, the rest from the JSON body, then validates it.
    async fn before(&self, job: &mut Job) -> Result<()> {
        // A direct invocation is the invoke protocol's to read.
        if job.request().extensions.get::<Invocation>().is_some() || job.input().is_some() {
            return Ok(());
        }
        let Some(op) = job.operation() else {
            return Err(FrameworkError::UnknownOperation.into());
        };
        let value = read_input(op, job)?;
        let input = job.decode(value)?;
        job.set_input(input);
        Ok(())
    }

    fn as_protocol(&self) -> Option<&dyn Protocol> {
        Some(self)
    }
}

fn read_input(op: &OperationInfo, job: &Job) -> std::result::Result<Value, Rejection> {
    let mut object =
        if op.input.iter().any(|m| m.location == Location::Body) && !job.body().is_empty() {
            match serde_json::from_slice::<Value>(job.body()) {
                Ok(Value::Object(object)) => object,
                Ok(_) => return Err(Rejection::malformed("", "the body must be a JSON object")),
                Err(error) => {
                    return Err(Rejection::malformed(
                        "",
                        format!("the body is not valid JSON: {error}"),
                    ));
                }
            }
        } else {
            Map::new()
        };
    let query = job
        .request()
        .uri
        .query()
        .map(parse_query)
        .unwrap_or_default();
    let mut violations = Vec::new();
    for member in op.input {
        let texts: Vec<String> = match member.location {
            Location::Body => continue,
            Location::Label => job
                .labels()
                .iter()
                .filter(|(name, _)| name == member.name)
                .map(|(_, value)| value.clone())
                .collect(),
            Location::Query(param) => query
                .iter()
                .filter(|(key, _)| key == param)
                .map(|(_, v)| v.clone())
                .collect(),
            Location::Header(name) => match job.request().headers.get(name) {
                None => Vec::new(),
                Some(value) => match value.to_str() {
                    Ok(text) if matches!(member.scalar, Scalar::List(_)) => split_header(text),
                    Ok(text) => vec![text.to_owned()],
                    Err(_) => {
                        violations.push(malformed(name, "is not visible ASCII"));
                        continue;
                    }
                },
            },
        };
        let wire = wire_name(member);
        let value = match member.scalar {
            Scalar::List(item) if !texts.is_empty() => texts
                .iter()
                .map(|text| from_text(text, *item))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map(Value::Array),
            Scalar::List(_) => continue,
            scalar => match texts.first() {
                Some(text) => from_text(text, scalar),
                None => continue,
            },
        };
        match value {
            Ok(value) => {
                object.insert(member.name.to_owned(), value);
            }
            Err(message) => violations.push(malformed(wire, &message)),
        }
    }
    if violations.is_empty() {
        Ok(Value::Object(object))
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

fn from_text(text: &str, scalar: Scalar) -> std::result::Result<Value, String> {
    match scalar {
        Scalar::String | Scalar::Json => Ok(Value::String(text.to_owned())),
        Scalar::Integer => text
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| format!("must be an integer (`{text}` is not)")),
        Scalar::Float | Scalar::EpochSeconds => text
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| format!("must be a number (`{text}` is not)")),
        Scalar::Boolean => match text {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err("must be true or false".to_owned()),
        },
        Scalar::DateTime => time::parse_date_time(text)
            .map(|_| Value::String(text.to_owned()))
            .ok_or_else(|| "must be an RFC 3339 timestamp".to_owned()),
        Scalar::HttpDate => time::http_date::parse(text)
            .map(|_| Value::String(text.to_owned()))
            .ok_or_else(|| "must be an HTTP date".to_owned()),
        Scalar::List(_) => Err("a list cannot hold a list here".to_owned()),
    }
}

fn to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => join_header(items.iter().map(to_text)),
        other => other.to_string(),
    }
}

/// Moves header members out of a serialized structure: the header pairs, and
/// what is left for the body.
fn split_members(members: &[MemberInfo], value: Value) -> (Vec<(&'static str, String)>, Value) {
    let Value::Object(mut object) = value else {
        return (Vec::new(), value);
    };
    let mut headers = Vec::new();
    for member in members {
        if let Location::Header(name) = member.location {
            if let Some(value) = object.remove(member.name).filter(|v| !v.is_null()) {
                headers.push((name, to_text(&value)));
            }
        }
    }
    (headers, Value::Object(object))
}

fn error_response(error: &dyn ModelledError) -> Response {
    let info = error.info();
    let (headers, mut body) = split_members(info.members, error.to_json());
    if let Value::Object(object) = &mut body {
        object.insert("__type".to_owned(), Value::String(info.name.to_owned()));
    }
    respond(info.status, headers, Some(body))
}

fn framework_response(error: &FrameworkError) -> Response {
    let body = serde_json::json!({ "__type": error.name(), "message": error.message() });
    respond(error.status(), Vec::new(), Some(body))
}

fn respond(status: u16, headers: Vec<(&'static str, String)>, body: Option<Value>) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = match body {
        Some(body) => (
            status,
            [(header::CONTENT_TYPE, "application/json")],
            body.to_string(),
        )
            .into_response(),
        None => (status, Body::empty()).into_response(),
    };
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            response.headers_mut().insert(name, value);
        }
    }
    response
}
