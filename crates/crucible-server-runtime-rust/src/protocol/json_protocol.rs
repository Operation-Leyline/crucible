//! `crucible.protocols#json`: members bound to the
//! URI path, query string and headers, the rest in a JSON object body, or a
//! blob payload as the whole body, raw; errors carry `"__type"`.

use axum::http::request::Parts;
use axum::response::Response;
use std::borrow::Cow;

use super::{Answers, BoundValue, Invocation, Protocol, ProtocolMatch, Route};
use crate::error::{FrameworkError, Result};
use crate::operation::{Location, MemberInfo, OperationInfo, Scalar, Segment};
use crate::operations::Operations;
use crate::validate::{Rejection, Violation, ViolationKind, Violations};
use crate::{Hook, Job};
use crucible_wire::text::{percent_decode, query_values};

#[derive(Debug, Clone, Copy, Default)]
pub struct JsonProtocol;

impl Protocol for JsonProtocol {
    fn route(&self, request: &Parts, operations: &Operations) -> Option<Route> {
        self.route_scoped(request, operations, &|_| true)
    }

    fn route_scoped(
        &self,
        request: &Parts,
        operations: &Operations,
        applies: &dyn Fn(&OperationInfo) -> bool,
    ) -> Option<Route> {
        route_candidates(request, operations, applies).0
    }

    fn identify_scoped<'a>(
        &'a self,
        job: &'a mut Job,
        operations: &'a Operations,
        applies: &'a (dyn Fn(&OperationInfo) -> bool + Send + Sync),
    ) -> Hook<'a, Result<ProtocolMatch>> {
        let (route, allowed) = route_candidates(job.request(), operations, applies);
        let matched = match route {
            Some(route) => Ok(ProtocolMatch::Claimed(Some(route))),
            None if self.route(job.request(), operations).is_some() => Ok(ProtocolMatch::Declined),
            None if !allowed.is_empty() => Err(FrameworkError::MethodNotAllowed { allowed }.into()),
            None => Ok(ProtocolMatch::Declined),
        };
        Hook::Ready(matched)
    }

    fn decode<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        decode_http(job)
    }

    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response {
        super::json_response::write(job, answers)
    }

    fn supports_server_stream(&self) -> bool {
        true
    }

    fn write_stream(&self, job: &Job, _: &Answers<'_>) -> Response {
        Response::builder()
            .status(job.operation().map_or(200, |op| op.status))
            .header(
                "content-type",
                "text/event-stream; charset=utf-8; crucible-stream=1",
            )
            .header("crucible-stream", "1")
            .header("cache-control", "no-store")
            .body(job.stream_body().expect("streaming outcome"))
            .expect("static stream headers")
    }

    fn stream_item(
        &self,
        item: &dyn crate::operation::Output,
        limit: usize,
    ) -> Result<axum::body::Bytes> {
        let mut sink = BoundedEvent {
            bytes: b"event: message\ndata: ".to_vec(),
            limit: limit.saturating_sub(2),
        };
        item.write_json_to(&mut sink).map_err(|_| {
            crate::Error::internal("stream item is invalid or exceeds the byte limit")
        })?;
        sink.bytes.extend_from_slice(b"\n\n");
        Ok(sink.bytes.into())
    }

    fn stream_heartbeat(&self) -> Option<axum::body::Bytes> {
        Some(axum::body::Bytes::from_static(b": heartbeat\n\n"))
    }

    fn stream_end(
        &self,
        job: &Job,
        answers: &Answers<'_>,
        limit: usize,
    ) -> Result<axum::body::Bytes> {
        if matches!(job.resolution(), Some(crate::Resolution::Streaming)) {
            return Ok(axum::body::Bytes::from_static(
                b"event: crucible.complete\ndata: {}\n\n",
            ));
        }
        let owned = match job.resolution() {
            Some(crate::Resolution::Rejected(rejection)) => Some((answers.reject)(rejection)),
            Some(crate::Resolution::Internal(_)) => answers.internal.map(|make| make()),
            Some(crate::Resolution::Framework(crate::FrameworkError::UnknownOperation)) => {
                answers.unknown.map(|make| make())
            }
            _ => None,
        };
        let modeled = owned.as_deref().or_else(|| match job.resolution() {
            Some(crate::Resolution::Modelled(error)) => Some(error),
            _ => None,
        });
        if modeled.is_none()
            && matches!(
                job.resolution(),
                Some(crate::Resolution::Internal(_)) | None
            )
        {
            return Ok(axum::body::Bytes::from_static(b"event: crucible.error\ndata: {\"type\":\"InternalFailure\",\"status\":500,\"error\":{\"message\":\"Internal server error\"}}\n\n"));
        }
        let mut sink = BoundedEvent {
            bytes: b"event: crucible.error\ndata: {\"type\":".to_vec(),
            limit: limit.saturating_sub(3),
        };
        let encoded = (|| -> serde_json::Result<()> {
            use std::io::Write;
            match (modeled, job.resolution()) {
                (Some(error), _) => {
                    serde_json::to_writer(&mut sink, error.info().name)?;
                    write!(sink, ",\"status\":{},\"error\":", error.info().status)
                        .map_err(serde_json::Error::io)?;
                    error.write_json_to(&mut sink)?;
                }
                (_, Some(crate::Resolution::Framework(error))) => {
                    serde_json::to_writer(&mut sink, error.name())?;
                    write!(sink, ",\"status\":{},\"error\":", error.status())
                        .map_err(serde_json::Error::io)?;
                    let mut fields = serde_json::json!({"message":error.message()});
                    if let crate::FrameworkError::NotFound { resource } = error {
                        fields["resourceType"] = resource.name.into();
                    }
                    serde_json::to_writer(&mut sink, &fields)?;
                }
                _ => {
                    return Err(serde_json::Error::io(std::io::Error::other(
                        "internal failure",
                    )));
                }
            }
            Ok(())
        })();
        if encoded.is_err() {
            return Err(crate::Error::internal(
                "terminal stream error is invalid or exceeds byte limit",
            ));
        }
        sink.bytes.extend_from_slice(b"}\n\n");
        Ok(sink.bytes.into())
    }
}

struct BoundedEvent {
    bytes: Vec<u8>,
    limit: usize,
}
impl std::io::Write for BoundedEvent {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("event exceeds byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn decode_http(job: &mut Job) -> Hook<'_, Result<()>> {
    Hook::Ready(check_content_type(job).and_then(|()| read_input(job)))
}

fn check_content_type(job: &Job) -> Result<()> {
    if job.body().is_empty() || job.request().extensions.get::<Invocation>().is_some() {
        return Ok(());
    }
    let op = job.operation().ok_or(FrameworkError::UnknownOperation)?;
    let payload = Location::payload(op.input);
    // Crucible's payload contract accepts raw bytes with any media type.
    if payload.is_some_and(|(member, _)| !matches!(member.location, Location::DocumentPayload(_))) {
        return Ok(());
    }
    // The document media type also applies to inputs whose members are all
    // HTTP-bound: clients may still send an empty JSON object for those inputs.
    let expected = payload.map_or("application/json", |(_, media_type)| media_type);
    if job.request().headers.get_all("content-type").iter().count() != 1
        || !super::content_type(job.request(), expected)
    {
        return Err(FrameworkError::UnsupportedMediaType.into());
    }
    Ok(())
}

fn route_candidates(
    request: &Parts,
    operations: &Operations,
    applies: &dyn Fn(&OperationInfo) -> bool,
) -> (Option<Route>, Vec<&'static str>) {
    if request.extensions.get::<Invocation>().is_some() {
        return (None, Vec::new());
    }
    let Some(rest) = request.uri.path().strip_prefix('/') else {
        return (None, Vec::new());
    };
    let rest = rest.trim_end_matches('/');
    let query = request.uri.query().unwrap_or("");
    // The methods allowed at the path only matter when no operation answers
    // (a 405), so they are gathered by a second pass in that case alone.
    let route = best_route(request, operations, applies, rest, query, None);
    if route.is_some() {
        return (route, Vec::new());
    }
    let mut allowed = Vec::new();
    best_route(
        request,
        operations,
        applies,
        rest,
        query,
        Some(&mut allowed),
    );
    allowed.sort_unstable();
    allowed.dedup();
    (None, allowed)
}

/// The path's segments as byte ranges of it: none for an empty path. Ranges
/// rather than a collected list, so routing allocates nothing for the path
/// and a greedy label is one slice of it.
struct Segments<'a> {
    path: &'a str,
    at: usize,
    done: bool,
}

impl<'a> Segments<'a> {
    fn new(path: &'a str) -> Self {
        Self {
            path,
            at: 0,
            done: path.is_empty(),
        }
    }
}

impl Iterator for Segments<'_> {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<(usize, usize)> {
        if self.done {
            return None;
        }
        let start = self.at;
        match self.path[start..].find('/') {
            Some(length) => {
                self.at = start + length + 1;
                Some((start, start + length))
            }
            None => {
                self.done = true;
                Some((start, self.path.len()))
            }
        }
    }
}

/// The most specific operation serving the request. With `allowed`, it
/// also collects the methods every operation at the path accepts.
fn best_route(
    request: &Parts,
    operations: &Operations,
    applies: &dyn Fn(&OperationInfo) -> bool,
    rest: &str,
    query: &str,
    mut allowed: Option<&mut Vec<&'static str>>,
) -> Option<Route> {
    let segments = Segments::new(rest).count();
    // The best candidate, its labels' violations (one at most per label, so
    // few), and whether its literal query matched.
    let mut best: Option<(&OperationInfo, Route, Vec<Violation>, bool)> = None;
    for &op in operations
        .routes(segments)
        .iter()
        .chain(operations.greedy_routes())
    {
        if !applies(op) {
            continue;
        }
        let exact_method = op.method == request.method.as_str();
        let method_matches = exact_method || request.method == "HEAD" && op.method == "GET";
        // Only the 405 pass needs the paths of other methods.
        if !method_matches && allowed.is_none() {
            continue;
        }
        let query_matches = match literal_queries_match(query, op) {
            Some(true) => true,
            Some(false) => continue,
            None => false,
        };
        if segments < op.path.len() {
            continue;
        }
        let mut labels = Vec::new();
        let mut violations = Vec::new();
        let mut parts = Segments::new(rest);
        let matched = op.path.iter().all(|segment| {
            let Some((start, end)) = parts.next() else {
                return false;
            };
            let part = &rest[start..end];
            match segment {
                Segment::Literal(text) => literal_segment_matches(text, part),
                Segment::Label(name) => {
                    if part.is_empty() {
                        return false;
                    }
                    match percent_decode(part, false) {
                        Ok(value) => labels.push((*name, value.into_owned())),
                        Err(message) => violations.push(malformed(name, &message)),
                    }
                    true
                }
                Segment::GreedyLabel(name) => {
                    // The segments it spans, joined by their slashes, are
                    // the path from its first to its last.
                    let mut last = end;
                    for _ in 1..segments - op.path.len() + 1 {
                        match parts.next() {
                            Some((_, end)) => last = end,
                            None => return false,
                        }
                    }
                    let joined = &rest[start..last];
                    if joined.is_empty() {
                        return false;
                    }
                    match percent_decode(joined, false) {
                        Ok(value) => labels.push((*name, value.into_owned())),
                        Err(message) => violations.push(malformed(name, &message)),
                    }
                    true
                }
                _ => false,
            }
        }) && parts.next().is_none();
        if !matched {
            continue;
        }
        if query_matches && let Some(allowed) = allowed.as_deref_mut() {
            allowed.push(op.method);
            if op.method == "GET" {
                allowed.push("HEAD");
            }
        }
        if !method_matches {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(most, _, _, most_query_matches)| {
                let previous_exact = most.method == request.method.as_str();
                (exact_method && !previous_exact)
                    || (exact_method == previous_exact
                        && more_specific(op, most, query_matches, *most_query_matches))
            })
        {
            best = Some((
                op,
                Route {
                    operation: op.name,
                    labels,
                    rejection: None,
                },
                violations,
                query_matches,
            ));
        }
    }
    // The query is the same for every candidate: it is checked once, for
    // the operation chosen, which also says what its parameters are called.
    best.map(|(op, mut route, labels, _)| {
        let mut violations = query_violations(query, op);
        violations.extend(labels);
        route.rejection = violations.into_rejection();
        route
    })
}

// Decode within a segment so an encoded separator never changes path structure.
fn literal_segment_matches(literal: &str, segment: &str) -> bool {
    match (
        percent_decode(literal, false),
        percent_decode(segment, false),
    ) {
        (Ok(literal), Ok(segment)) => literal == segment,
        _ => false,
    }
}

/// Path segments decide specificity before query constraints. A later
/// literal must not outweigh an earlier literal or non-greedy label.
fn more_specific(
    candidate: &OperationInfo,
    previous: &OperationInfo,
    candidate_query_matches: bool,
    previous_query_matches: bool,
) -> bool {
    let rank = |segment: &Segment| match segment {
        Segment::Literal(_) => 2,
        Segment::Label(_) => 1,
        Segment::GreedyLabel(_) => 0,
        _ => -1,
    };
    candidate
        .path
        .iter()
        .map(rank)
        .cmp(previous.path.iter().map(rank))
        .then_with(|| candidate_query_matches.cmp(&previous_query_matches))
        .then_with(|| {
            candidate
                .literal_query
                .len()
                .cmp(&previous.literal_query.len())
        })
        .then_with(|| {
            let exact = |operation: &OperationInfo| {
                operation
                    .literal_query
                    .iter()
                    .filter(|(_, value)| value.is_some())
                    .count()
            };
            exact(candidate).cmp(&exact(previous))
        })
        .is_gt()
}

/// Reads the input: bound members from the request's labels, query and
/// headers, the rest from the JSON body, then validates it. A payload is the
/// body itself, unparsed, whatever its content type.
pub(super) fn read_input(job: &mut Job) -> Result<()> {
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
pub(super) fn bound_members<'a>(
    op: &OperationInfo,
    job: &'a Job,
) -> std::result::Result<Vec<(&'static str, BoundValue<'a>)>, Rejection> {
    let query = job.request().uri.query().unwrap_or("");
    let mut bound = Vec::new();
    let mut violations = Violations::default();
    for member in op.input {
        let wire = wire_name(member);
        let value = match (member.location, member.scalar) {
            (Location::Body, _) => continue,
            // An empty body is no payload: a required one is missing.
            (Location::Payload(_), _) if job.body().is_empty() => continue,
            (Location::Payload(_), _) => Ok(BoundValue::Bytes(job.body().clone())),
            (Location::TextPayload(_), _) => std::str::from_utf8(job.body())
                .map(|text| BoundValue::Text(Cow::Borrowed(text)))
                .map_err(|_| "must be UTF-8 text".to_owned()),
            (Location::DocumentPayload(_), _)
                if crucible_wire::text::empty_json_body(job.body()) =>
            {
                continue;
            }
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
                        .map(|text| text.and_then(|text| BoundValue::parse(text, *item)))
                        .collect();
                match items {
                    Ok(items) if items.is_empty() => continue,
                    other => other.map(BoundValue::List),
                }
            }
            (Location::Query(param), scalar) => match query_values(query, param).next() {
                Some(text) => text.and_then(|text| BoundValue::parse(text, scalar)),
                None => continue,
            },
            (Location::Header(name), scalar @ Scalar::List(_)) => {
                let mut fields = job.request().headers.get_all(name).iter().peekable();
                if fields.peek().is_none() {
                    continue;
                }
                let mut items = Vec::new();
                let result = fields.try_for_each(|value| {
                    let text = value
                        .to_str()
                        .map_err(|_| "is not visible ASCII".to_owned())?;
                    let BoundValue::List(field_items) =
                        BoundValue::parse(Cow::Borrowed(text), scalar)?
                    else {
                        unreachable!("list scalar parses as a list");
                    };
                    items.extend(field_items);
                    Ok::<_, String>(())
                });
                result.map(|()| BoundValue::List(items))
            }
            (Location::Header(name), scalar) => {
                let Some(value) = job.request().headers.get(name) else {
                    continue;
                };
                let Ok(text) = value.to_str() else {
                    violations.push_with(|| malformed(name, "is not visible ASCII"));
                    continue;
                };
                BoundValue::parse(Cow::Borrowed(text), scalar)
            }
            _ => Err("unsupported member binding".to_owned()),
        };
        match value {
            Ok(value) => bound.push((member.name, value)),
            Err(message) => violations.push_with(|| malformed(wire, &message)),
        }
    }
    match violations.into_rejection() {
        None => Ok(bound),
        Some(rejection) => Err(rejection),
    }
}

/// The name a bound member goes by on the wire, for a violation's path.
fn wire_name(member: &MemberInfo) -> &'static str {
    match member.location {
        Location::Query(name) | Location::Header(name) => name,
        _ => member.name,
    }
}

fn malformed(path: &str, message: &str) -> Violation {
    Violation {
        path: path.to_owned(),
        message: message.to_owned(),
        kind: ViolationKind::Malformed,
    }
}

/// True for matched literals, false for a known mismatch or missing literal,
/// and undecidable when the relevant key or value could not be decoded.
fn literal_queries_match(query: &str, operation: &OperationInfo) -> Option<bool> {
    let mut fully_matched = true;
    for (key, expected) in operation.literal_query {
        let mut key_seen = false;
        let mut malformed_key = false;
        let mut malformed_value = false;
        let mut matched = false;
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let name = match percent_decode(name, true) {
                Ok(name) => name,
                Err(_) => {
                    malformed_key = true;
                    continue;
                }
            };
            if name != *key {
                continue;
            }
            key_seen = true;
            match percent_decode(value, true) {
                Ok(value) if expected.is_none_or(|expected| value == expected) => {
                    matched = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => malformed_value = true,
            }
        }
        if matched {
            continue;
        }
        // Once the key is readable, unrelated malformed keys cannot erase
        // its valid mismatch. Repeated matching values retain any-match semantics.
        if malformed_value || (!key_seen && malformed_key) {
            fully_matched = false;
        } else {
            return Some(false);
        }
    }
    fully_matched.then_some(true)
}

/// Every query component, checked before routing on literal keys or
/// values (a malformed component must not hide a more specific route), and
/// reported against the operation chosen. A parameter it declares goes by
/// the model's name; any other key is the client's text, so it is never
/// named.
fn query_violations(query: &str, op: &OperationInfo) -> Violations {
    let mut violations = Violations::default();
    if !query.contains('%') {
        return violations;
    }
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match percent_decode(key, true) {
            Ok(key) => {
                if let Err(message) = percent_decode(value, true) {
                    violations.push_with(|| malformed(&query_path(op, Some(&key)), &message));
                }
            }
            Err(message) => violations.push_with(|| malformed(&query_path(op, None), &message)),
        }
    }
    violations
}

/// The path naming query parameter `key` of `op`, or a key that does not
/// decode (`None`).
fn query_path(op: &OperationInfo, key: Option<&str>) -> Cow<'static, str> {
    let declared = op
        .literal_query
        .iter()
        .map(|(name, _)| *name)
        .chain(op.input.iter().filter_map(|member| match member.location {
            Location::Query(name) => Some(name),
            _ => None,
        }))
        .find(|name| key == Some(*name));
    if let Some(name) = declared {
        return Cow::Borrowed(name);
    }
    Cow::Borrowed("query")
}
