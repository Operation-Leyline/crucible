//! JSON response serialization for the Crucible JSON protocol.
use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

use super::Answers;
use crate::error::{FrameworkError, ModelledError};
use crate::operation::Location;
use crate::{Job, Resolution};

const CONTENT_TYPE: &str = "application/json";

pub(super) fn write(job: &Job, answers: &Answers<'_>) -> Response {
    write_outcome(job, answers)
}

fn write_outcome(job: &Job, answers: &Answers<'_>) -> Response {
    let operation = job.operation();
    match job.resolution() {
        Some(Resolution::Success(output)) => {
            let op = operation.expect("an output implies an operation");
            if let Some((_, media_type)) = Location::payload(op.output) {
                // The payload's bytes are the body, under its own media type.
                let mut body = Vec::new();
                return match output.write_json_split(op.output, &mut body) {
                    Ok(headers) => respond(
                        op.status,
                        owned(headers),
                        (!body.is_empty()).then_some((media_type, body)),
                    ),
                    Err(error) => serialization_failure(error),
                };
            }
            let has_body = op.output.iter().any(|m| m.location == Location::Body);
            let has_headers = op
                .output
                .iter()
                .any(|m| matches!(m.location, Location::Header(_)));
            if !has_headers {
                // Straight to bytes, with no JSON value in between.
                if !has_body {
                    return respond(op.status, Vec::new(), None);
                }
                let mut body = Vec::with_capacity(256);
                return match output.write_json(&mut body) {
                    Ok(()) => respond(op.status, Vec::new(), Some((CONTENT_TYPE, body))),
                    Err(error) => serialization_failure(error),
                };
            }
            // Header members leave the body as it is written.
            let mut body = Vec::with_capacity(256);
            match output.write_json_split(op.output, &mut body) {
                Ok(headers) => respond(
                    op.status,
                    owned(headers),
                    has_body.then_some((CONTENT_TYPE, body)),
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

fn error_response(error: &dyn ModelledError) -> Response {
    let mut body = Vec::with_capacity(128);
    match error.write_json(&mut body) {
        Ok(headers) => {
            let mut headers = owned(headers);
            headers.push(("x-crucible-error".to_owned(), error.info().name.to_owned()));
            let media_type = Location::payload(error.info().members)
                .map_or(CONTENT_TYPE, |(_, media_type)| media_type);
            respond(error.info().status, headers, Some((media_type, body)))
        }
        Err(error) => serialization_failure(error),
    }
}

fn framework_response(error: &FrameworkError) -> Response {
    if let FrameworkError::NotFound { resource } = error {
        return not_found(error, resource.name);
    }
    let body = serde_json::json!({ "__type": error.name(), "message": error.message() });
    // The discriminator, as a modelled error sends it: clients read it
    // before the body's `__type`.
    let mut headers = vec![("x-crucible-error".to_owned(), error.name().to_owned())];
    if let FrameworkError::MethodNotAllowed { allowed } = error {
        headers.push(("allow".to_owned(), allowed.join(", ")));
    }
    respond(
        error.status(),
        headers,
        Some((CONTENT_TYPE, to_bytes(&body))),
    )
}

/// [`FrameworkError::NotFound`], encoded as the modelled
/// `crucible.errors#NotFoundException` the compiler attaches to every
/// resource-bound operation, so an SDK reads it as that typed error: named
/// as a modelled error, with its `resourceType` member,
/// and never cached, since whether a resource is visible changes with
/// grants.
fn not_found(error: &FrameworkError, resource_type: &str) -> Response {
    let body = serde_json::json!({
        "__type": error.name(),
        "message": error.message(),
        "resourceType": resource_type,
    });
    let mut headers = vec![("cache-control".to_owned(), "no-store".to_owned())];
    headers.push(("x-crucible-error".to_owned(), error.name().to_owned()));
    respond(
        error.status(),
        headers,
        Some((CONTENT_TYPE, to_bytes(&body))),
    )
}

fn owned(headers: Vec<(&'static str, String)>) -> Vec<(String, String)> {
    headers
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

fn serialization_failure(error: impl std::fmt::Display) -> Response {
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
    if content_type == "application/json" {
        HeaderValue::from_static("application/json")
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
        let name = match name {
            Ok(name) => name,
            Err(_) => return serialization_failure("invalid response header name"),
        };
        let value = match HeaderValue::try_from(value) {
            Ok(value) => value,
            Err(_) => {
                return serialization_failure(format!("invalid response header value for {name}"));
            }
        };
        response.headers_mut().insert(name, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::framework_response;
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
    async fn a_conflict_is_a_409() {
        let conflict = FrameworkError::Conflict {
            message: "the token was used for a different request",
        };
        let response = framework_response(&conflict);
        assert_eq!(response.status().as_u16(), 409);
        let body = body(response).await;
        assert_eq!(body["__type"], "ConflictException");
        assert_eq!(body["message"], conflict.message());
    }

    #[test]
    fn every_framework_error_is_named_by_its_header() {
        let errors = [
            FrameworkError::Unauthenticated,
            FrameworkError::Forbidden,
            FrameworkError::UnknownOperation,
            FrameworkError::MethodNotAllowed {
                allowed: vec!["GET"],
            },
            FrameworkError::UnsupportedMediaType,
            FrameworkError::InternalFailure,
            FrameworkError::Conflict { message: "taken" },
            FrameworkError::NotFound { resource: &INBOX },
            FrameworkError::PermissionDenied { permission: &SEND },
        ];
        for error in &errors {
            let response = framework_response(error);
            let header = response
                .headers()
                .get("x-crucible-error")
                .map(|value| value.to_str().unwrap());
            assert_eq!(header, Some(error.name()), "{error:?}");
        }
    }

    async fn body(response: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn not_found_is_the_modelled_404_and_never_cached() {
        let missing = FrameworkError::NotFound { resource: &INBOX };
        let response = framework_response(&missing);
        assert_eq!(response.status().as_u16(), 404);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["x-crucible-error"], "NotFoundException");
        assert_eq!(
            body(response).await,
            serde_json::json!({
                "__type": "NotFoundException",
                "message": "No such inbox.",
                "resourceType": "inbox",
            })
        );
    }

    #[tokio::test]
    async fn a_permission_refusal_is_a_403_forbidden_exception() {
        let denied = FrameworkError::PermissionDenied { permission: &SEND };
        let response = framework_response(&denied);
        assert_eq!(response.status().as_u16(), 403);
        assert!(!response.headers().contains_key("www-authenticate"));
        let body = body(response).await;
        assert_eq!(body["__type"], "ForbiddenException");
        assert_eq!(
            body["message"],
            "This principal does not hold mail:send on that inbox."
        );
    }
}
