use axum::http::request::Parts;

use super::{Invocation, ProtocolMatch, Route};
use crate::operation::OperationInfo;
use crate::validate::Rejection;
use crate::{Job, Operations, Result};

pub(super) fn content_type(request: &Parts, expected: &str) -> bool {
    request
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case(expected)
        })
}

pub(super) fn owns(request: &Parts, protocol: &str) -> bool {
    request.extensions.get::<Invocation>().is_none()
        && request
            .headers
            .get("smithy-protocol")
            .and_then(|v| v.to_str().ok())
            == Some(protocol)
}

pub(super) fn identify(
    job: &Job,
    operations: &Operations,
    applies: &dyn Fn(&OperationInfo) -> bool,
    protocol: &str,
    media_type: &str,
) -> Result<ProtocolMatch> {
    let request = job.request();
    if !owns(request, protocol) {
        return Ok(ProtocolMatch::Declined);
    }
    let bad = |message| Rejection::malformed("", message);
    if request.method != "POST" {
        return Err(bad("RPC v2 requests require POST").into());
    }
    if request.headers.get_all("smithy-protocol").iter().count() != 1 {
        return Err(bad("smithy-protocol must occur once").into());
    }
    let mut segments = request.uri.path().rsplit('/');
    let operation = segments.next().unwrap_or("");
    let operation_slot = segments.next();
    let service = segments.next().unwrap_or("");
    let service_slot = segments.next();
    if operation.is_empty()
        || operation_slot != Some("operation")
        || service_slot != Some("service")
    {
        return Err(bad("RPC v2 path requires /service/Service/operation/Operation").into());
    }
    let id = operations.service().id;
    let name = id.rsplit_once('#').map_or(id, |(_, name)| name);
    let full_name = id.rsplit_once('#').is_some_and(|(namespace, name)| {
        service
            .strip_prefix(namespace)
            .and_then(|value| value.strip_prefix('.'))
            == Some(name)
    });
    if service != name && !full_name {
        return Ok(ProtocolMatch::Claimed(None));
    }
    let Some(op) = operations.service().operation(operation) else {
        return Ok(ProtocolMatch::Claimed(None));
    };
    if !applies(op) {
        return Ok(ProtocolMatch::Declined);
    }
    // Not served at this API version, though its release declares it.
    let Some(metadata) = operations.metadata(op.name) else {
        return Ok(ProtocolMatch::Claimed(None));
    };
    let media_type = if metadata.input_events.is_some() {
        crate::event_stream::CONTENT_TYPE
    } else {
        media_type
    };
    let rejection = if request.headers.contains_key("x-amz-target")
        || request.headers.contains_key("x-amzn-target")
    {
        Some(bad("RPC v2 forbids AWS target headers"))
    } else if metadata.input_events.is_some() && request.headers.contains_key("content-length") {
        Some(bad("RPC v2 event streams forbid content-length"))
    } else if metadata.has_input && !content_type(request, media_type) {
        Some(bad("RPC v2 request has an incompatible content-type"))
    } else {
        None
    };
    Ok(ProtocolMatch::Claimed(Some(Route {
        operation: op.name,
        labels: Vec::new(),
        rejection,
    })))
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
