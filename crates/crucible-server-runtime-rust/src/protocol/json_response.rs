//! Shared JSON response serialization; protocol differences are confined to the format.
use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

use super::Answers;
use crate::error::{FrameworkError, ModelledError};
use crate::operation::Location;
use crate::{Job, Resolution};

#[derive(Debug, Clone, Copy)]
pub(super) enum Format {
    Crucible,
    RestJson,
    AwsJson10,
    AwsJson11,
    RpcV2Json,
    RpcV2Cbor,
}

impl Format {
    fn content_type(self) -> &'static str {
        match self {
            Self::Crucible | Self::RestJson | Self::RpcV2Json => "application/json",
            Self::AwsJson10 => "application/x-amz-json-1.0",
            Self::AwsJson11 => "application/x-amz-json-1.1",
            Self::RpcV2Cbor => "application/cbor",
        }
    }
    fn aws(self) -> bool {
        matches!(self, Self::RestJson | Self::AwsJson10 | Self::AwsJson11)
    }
    fn rpc_v2(self) -> bool {
        matches!(self, Self::RpcV2Json | Self::RpcV2Cbor)
    }
}

pub(super) fn write(job: &Job, answers: &Answers<'_>, format: Format) -> Response {
    let mut response = write_outcome(job, answers, format);
    if format.aws() {
        response.headers_mut().insert(
            "x-amzn-requestid",
            job.context().request_id_header().clone(),
        );
    }
    if format.rpc_v2() {
        response.headers_mut().insert(
            "smithy-protocol",
            HeaderValue::from_static(if matches!(format, Format::RpcV2Cbor) {
                "rpc-v2-cbor"
            } else {
                "rpc-v2-json"
            }),
        );
    }
    response
}

fn write_outcome(job: &Job, answers: &Answers<'_>, format: Format) -> Response {
    let operation = job.operation();
    match job.resolution() {
        Some(Resolution::Success(output)) => {
            let op = operation.expect("an output implies an operation");
            if let Some(info) = job
                .context()
                .operation_metadata()
                .and_then(|m| m.output_events)
            {
                let events = match output
                    .events()
                    .unwrap_or_else(|| Ok(Box::pin(futures_util::stream::empty())))
                {
                    Ok(events) => events,
                    Err(error) => return serialization_failure(error, format),
                };
                let mut initial = Vec::new();
                let (status, headers) = if format.rpc_v2() {
                    let result = if matches!(format, Format::RpcV2Cbor) {
                        output.write_cbor(&mut initial)
                    } else {
                        output.write_json(&mut initial).map_err(|e| e.to_string())
                    };
                    if let Err(error) = result {
                        return serialization_failure(error, format);
                    }
                    (200, Vec::new())
                } else {
                    let bindings = match output.write_http(op.output, &mut initial) {
                        Ok(bindings) => bindings,
                        Err(error) => return serialization_failure(error, format),
                    };
                    (bindings.status.unwrap_or(op.status), bindings.headers)
                };
                let mut response = respond(format, status, headers, None);
                if response
                    .extensions()
                    .get::<super::SerializationFailure>()
                    .is_some()
                {
                    return response;
                }
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static(crate::event_stream::CONTENT_TYPE),
                );
                response.headers_mut().remove(header::CONTENT_LENGTH);
                *response.body_mut() = super::server_events::body(
                    info,
                    events,
                    info.initial_message.then(|| initial.into()),
                );
                return response;
            }
            if format.rpc_v2() {
                if job
                    .context()
                    .operation_metadata()
                    .is_some_and(|m| !m.has_output)
                {
                    return respond(format, 200, Vec::new(), None);
                }
                let mut body = Vec::with_capacity(256);
                let result = if matches!(format, Format::RpcV2Cbor) {
                    output.write_cbor(&mut body)
                } else {
                    output
                        .write_json(&mut body)
                        .map_err(|error| error.to_string())
                };
                return match result {
                    Ok(()) => respond(format, 200, Vec::new(), Some((format.content_type(), body))),
                    Err(error) => serialization_failure(error, format),
                };
            }
            if let Some(stream) = output.stream() {
                let mut ignored = Vec::new();
                let bindings = match output.write_http(op.output, &mut ignored) {
                    Ok(bindings) => bindings,
                    Err(error) => return serialization_failure(error, format),
                };
                let Some((member, media_type)) = Location::payload(op.output) else {
                    return framework_response(&FrameworkError::InternalFailure, format);
                };
                let stream = match member.location {
                    Location::StreamingPayload {
                        minimum, maximum, ..
                    } => stream.with_bounds(minimum, maximum),
                    _ => stream,
                };
                let mut response = respond(
                    format,
                    bindings.status.unwrap_or(op.status),
                    bindings.headers,
                    None,
                );
                if response
                    .extensions()
                    .get::<super::SerializationFailure>()
                    .is_some()
                {
                    return response;
                }
                return match stream.open() {
                    Ok(body) => {
                        response
                            .headers_mut()
                            .insert(header::CONTENT_TYPE, content_type_header(media_type));
                        if let Some(length) = stream.length() {
                            response.headers_mut().insert(
                                header::CONTENT_LENGTH,
                                HeaderValue::from_str(&length.to_string()).expect("integer header"),
                            );
                        }
                        *response.body_mut() = Body::from_stream(body);
                        response
                    }
                    Err(_) => framework_response(&FrameworkError::InternalFailure, format),
                };
            }
            if let Some((_, media_type)) = Location::payload(op.output) {
                // The payload's bytes are the body, under its own media type.
                let mut body = Vec::new();
                return match output.write_http(op.output, &mut body) {
                    Ok(bindings) => respond(
                        format,
                        bindings.status.unwrap_or(op.status),
                        bindings.headers,
                        (!body.is_empty()).then_some((media_type, body)),
                    ),
                    Err(error) => serialization_failure(error, format),
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
                    return respond(format, op.status, Vec::new(), None);
                }
                let mut body = Vec::with_capacity(256);
                return match output.write_json(&mut body) {
                    Ok(()) => respond(
                        format,
                        op.status,
                        Vec::new(),
                        Some((format.content_type(), body)),
                    ),
                    Err(error) => serialization_failure(error, format),
                };
            }
            // Header members leave the body as it is written.
            let mut body = Vec::with_capacity(256);
            match output.write_http(op.output, &mut body) {
                Ok(bindings) => respond(
                    format,
                    bindings.status.unwrap_or(op.status),
                    bindings.headers,
                    has_body.then_some((format.content_type(), body)),
                ),
                Err(error) => serialization_failure(error, format),
            }
        }
        Some(Resolution::Modelled(error)) => error_response(error, format),
        Some(Resolution::Rejected(rejection)) => {
            error_response(&*(answers.reject)(rejection), format)
        }
        Some(Resolution::Framework(FrameworkError::UnknownOperation))
            if answers.unknown.is_some() =>
        {
            error_response(&*(answers.unknown.expect("checked"))(), format)
        }
        Some(Resolution::Framework(error)) => framework_response(&error, format),
        Some(Resolution::Internal(_)) => match answers.internal {
            Some(internal) => error_response(&*internal(), format),
            None => framework_response(&FrameworkError::InternalFailure, format),
        },
        // An answered request never reaches the protocol.
        Some(Resolution::Answered { .. }) | None => {
            framework_response(&FrameworkError::InternalFailure, format)
        }
    }
}

fn error_response(error: &dyn ModelledError, format: Format) -> Response {
    let mut body = Vec::with_capacity(128);
    if matches!(format, Format::RpcV2Cbor) {
        return match error.write_cbor(&mut body) {
            Ok(()) => respond(
                format,
                error.info().status,
                Vec::new(),
                Some((format.content_type(), body)),
            ),
            Err(error) => serialization_failure(error, format),
        };
    }
    let written = match format {
        Format::Crucible => error.write_http(&mut body),
        Format::RestJson => error.write_http_typed(None, &mut body),
        Format::AwsJson10 => error.write_http_typed(Some(error.shape_id()), &mut body),
        Format::AwsJson11 => error.write_http_typed(Some(error.info().name), &mut body),
        Format::RpcV2Json => error.write_http_typed(Some(error.shape_id()), &mut body),
        Format::RpcV2Cbor => unreachable!("CBOR handled above"),
    };
    match written {
        Ok(mut bindings) => {
            if matches!(format, Format::Crucible) {
                bindings
                    .headers
                    .push(("x-crucible-error".to_owned(), error.info().name.to_owned()));
            }
            if format.aws() {
                bindings
                    .headers
                    .push(("x-amzn-errortype".to_owned(), error.info().name.to_owned()));
            }
            let media_type = Location::payload(error.info().members)
                .map_or(format.content_type(), |(_, media_type)| media_type);
            respond(
                format,
                bindings.status.unwrap_or(error.response_status()),
                bindings.headers,
                Some((media_type, body)),
            )
        }
        Err(error) => serialization_failure(error, format),
    }
}

fn framework_response(error: &FrameworkError, format: Format) -> Response {
    if let FrameworkError::NotFound { resource } = error {
        return not_found(error, resource.name, format);
    }
    let type_name = if format.rpc_v2() {
        match error {
            FrameworkError::Unauthenticated => "smithy.framework#UnauthenticatedException",
            FrameworkError::Forbidden | FrameworkError::PermissionDenied { .. } => {
                "smithy.framework#ForbiddenException"
            }
            FrameworkError::Conflict { .. } => "smithy.framework#ConflictException",
            FrameworkError::NotFound { .. } => NOT_FOUND_SHAPE,
            FrameworkError::UnknownOperation => "smithy.framework#UnknownOperationException",
            FrameworkError::MethodNotAllowed { .. } => "smithy.framework#MethodNotAllowedException",
            FrameworkError::UnsupportedMediaType => {
                "smithy.framework#UnsupportedMediaTypeException"
            }
            FrameworkError::InternalFailure => "smithy.framework#InternalFailureException",
        }
    } else {
        error.name()
    };
    let body = serde_json::json!({ "__type": type_name, "message": error.message() });
    let mut headers = if format.aws() {
        vec![("x-amzn-errortype".to_owned(), error.name().to_owned())]
    } else {
        Vec::new()
    };
    if let FrameworkError::MethodNotAllowed { allowed } = error {
        headers.push(("allow".to_owned(), allowed.join(", ")));
    }
    let mut bytes = Vec::new();
    if matches!(format, Format::RpcV2Cbor) {
        crate::cbor::write(&body, &mut bytes).expect("framework error is CBOR serializable");
    } else {
        bytes = to_bytes(&body);
    }
    respond(
        format,
        error.status(),
        headers,
        Some((format.content_type(), bytes)),
    )
}

/// The modelled error a hidden or missing resource is answered with.
const NOT_FOUND_SHAPE: &str = "crucible.errors#NotFoundException";

/// [`FrameworkError::NotFound`], encoded as the modelled
/// `crucible.errors#NotFoundException` the compiler attaches to every
/// resource-bound operation, so an SDK reads it as that typed error: named
/// as each protocol names a modelled error, with its `resourceType` member,
/// and never cached, since whether a resource is visible changes with
/// grants.
fn not_found(error: &FrameworkError, resource_type: &str, format: Format) -> Response {
    let type_name = match format {
        Format::AwsJson10 | Format::RpcV2Json | Format::RpcV2Cbor => NOT_FOUND_SHAPE,
        Format::Crucible | Format::RestJson | Format::AwsJson11 => error.name(),
    };
    let body = serde_json::json!({
        "__type": type_name,
        "message": error.message(),
        "resourceType": resource_type,
    });
    let mut headers = vec![("cache-control".to_owned(), "no-store".to_owned())];
    if matches!(format, Format::Crucible) {
        headers.push(("x-crucible-error".to_owned(), error.name().to_owned()));
    }
    if format.aws() {
        headers.push(("x-amzn-errortype".to_owned(), error.name().to_owned()));
    }
    let mut bytes = Vec::new();
    if matches!(format, Format::RpcV2Cbor) {
        crate::cbor::write(&body, &mut bytes).expect("framework error is CBOR serializable");
    } else {
        bytes = to_bytes(&body);
    }
    respond(
        format,
        error.status(),
        headers,
        Some((format.content_type(), bytes)),
    )
}

fn serialization_failure(error: impl std::fmt::Display, format: Format) -> Response {
    let mut response = framework_response(&FrameworkError::InternalFailure, format);
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
    if content_type == "application/json" {
        HeaderValue::from_static("application/json")
    } else {
        HeaderValue::from_str(content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream"))
    }
}

/// `body` is the content type and the bytes.
fn respond(
    format: Format,
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
        None => {
            let mut response = (status, Body::empty()).into_response();
            if format.aws() {
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static(format.content_type()),
                );
            }
            response
        }
    };
    for (name, value) in headers {
        // Generated header names are lowercase, so they parse without copying.
        let name =
            HeaderName::from_lowercase(name.as_bytes()).or_else(|_| HeaderName::try_from(name));
        let name = match name {
            Ok(name) => name,
            Err(_) => return serialization_failure("invalid response header name", format),
        };
        let value = match HeaderValue::try_from(value) {
            Ok(value) => value,
            Err(_) => {
                return serialization_failure(
                    format!("invalid response header value for {name}"),
                    format,
                );
            }
        };
        response.headers_mut().insert(name, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::{Format, framework_response};
    use crate::error::FrameworkError;
    use crate::permissions::{PermissionInfo, ResourceInfo, Tier};

    static INBOX: ResourceInfo = ResourceInfo {
        name: "inbox",
        identifiers: &["workspaceId", "inboxId"],
        not_found: "No such inbox.",
    };
    static SEND: PermissionInfo = PermissionInfo {
        resource: &INBOX,
        symbol: "mailSend",
        value: "mail:send",
        aliases: &[],
        tier: Tier::Public,
        denied: "This principal does not hold mail:send on that inbox.",
    };

    #[tokio::test]
    async fn a_conflict_is_a_409_named_for_each_protocol() {
        let conflict = FrameworkError::Conflict {
            message: "the token was used for a different request",
        };
        for (format, name) in [
            (Format::Crucible, "ConflictException"),
            (Format::RestJson, "ConflictException"),
            (Format::RpcV2Json, "smithy.framework#ConflictException"),
        ] {
            let response = framework_response(&conflict, format);
            assert_eq!(response.status().as_u16(), 409);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["__type"], name);
            assert_eq!(body["message"], conflict.message());
        }
    }

    async fn body(response: axum::response::Response, cbor: bool) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        if cbor {
            crate::cbor::read(&bytes).unwrap()
        } else {
            serde_json::from_slice(&bytes).unwrap()
        }
    }

    #[tokio::test]
    async fn not_found_is_the_modelled_404_in_every_protocol_and_never_cached() {
        let missing = FrameworkError::NotFound { resource: &INBOX };
        for (format, name, error_header) in [
            (
                Format::Crucible,
                "NotFoundException",
                Some("x-crucible-error"),
            ),
            (
                Format::RestJson,
                "NotFoundException",
                Some("x-amzn-errortype"),
            ),
            (
                Format::AwsJson10,
                "crucible.errors#NotFoundException",
                Some("x-amzn-errortype"),
            ),
            (
                Format::AwsJson11,
                "NotFoundException",
                Some("x-amzn-errortype"),
            ),
            (Format::RpcV2Json, "crucible.errors#NotFoundException", None),
            (Format::RpcV2Cbor, "crucible.errors#NotFoundException", None),
        ] {
            let response = framework_response(&missing, format);
            assert_eq!(response.status().as_u16(), 404, "{format:?}");
            assert_eq!(
                response.headers()["cache-control"],
                "no-store",
                "{format:?}"
            );
            if let Some(header) = error_header {
                assert_eq!(
                    response.headers()[header],
                    "NotFoundException",
                    "{format:?}"
                );
            }
            let body = body(response, matches!(format, Format::RpcV2Cbor)).await;
            assert_eq!(
                body,
                serde_json::json!({
                    "__type": name,
                    "message": "No such inbox.",
                    "resourceType": "inbox",
                }),
                "{format:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_permission_refusal_is_a_403_forbidden_exception() {
        let denied = FrameworkError::PermissionDenied { permission: &SEND };
        for (format, name) in [
            (Format::Crucible, "ForbiddenException"),
            (Format::RestJson, "ForbiddenException"),
            (Format::AwsJson11, "ForbiddenException"),
            (Format::RpcV2Json, "smithy.framework#ForbiddenException"),
        ] {
            let response = framework_response(&denied, format);
            assert_eq!(response.status().as_u16(), 403);
            assert!(!response.headers().contains_key("www-authenticate"));
            let body = body(response, false).await;
            assert_eq!(body["__type"], name);
            assert_eq!(
                body["message"],
                "This principal does not hold mail:send on that inbox."
            );
        }
    }
}
