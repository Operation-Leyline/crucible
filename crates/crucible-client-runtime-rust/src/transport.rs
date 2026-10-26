//! Sending an operation: the request built from the operation's
//! description, attempts retried by the rules in `docs/framework.md`, and
//! the response decoded into the output or the operation's error.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use crucible_wire::bound::{self, BoundValue};
use crucible_wire::operation::{ErrorInfo, Location, MemberInfo, Operation, Scalar, Segment};
use crucible_wire::split;
use crucible_wire::text::{header_text, scalar_text as text};
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::config::{CallOptions, Config};

/// The errors one operation can return, as a generated enum.
pub trait OperationErrors: Sized + fmt::Debug {
    /// The error a response names (`type_name`, from the body's `__type`),
    /// or the one the status names when the operation has only one with it.
    fn decode(
        status: u16,
        type_name: Option<&str>,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Option<Self>;

    fn info(&self) -> &'static ErrorInfo;

    /// Static documentation page for this modeled error and its known reason.
    fn doc_url(&self) -> Option<&'static str> {
        None
    }

    /// Code page retained when a known error's modeled fields cannot be decoded.
    fn classified_doc_url(_status: u16, _type_name: Option<&str>) -> Option<&'static str> {
        None
    }

    /// Identifies modeled metadata independently of decoding the error's fields.
    fn classify(_status: u16, _type_name: Option<&str>) -> Option<&'static ErrorInfo> {
        None
    }
}

/// Why a call failed.
#[derive(Debug)]
pub enum Error<E> {
    /// A modelled error the operation declares.
    Service(ServiceError<E>),
    /// An error response the client cannot name: an unmodelled one, or a
    /// newer service's.
    Unknown(UnknownError),
    /// No response: connection, TLS, timeout.
    Transport(reqwest::Error),
    /// The request could not be built from the input.
    Invalid(String),
    /// A response the client could not read as the model says.
    Decode(String),
    /// The total call budget expired. No further attempts are made.
    Timeout,
    /// The caller cancelled the operation.
    Cancelled,
}

/// HTTP metadata retained for every service error, including unknown shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseMetadata {
    pub status: u16,
    pub request_id: Option<String>,
    pub headers: HeaderMap,
}

#[derive(Debug)]
pub struct ServiceError<E> {
    pub error: E,
    pub metadata: Box<ResponseMetadata>,
}

impl<E: OperationErrors> ServiceError<E> {
    /// Static documentation for the modeled error code and known reason.
    pub fn doc_url(&self) -> Option<&'static str> {
        self.error.doc_url()
    }
}

impl<E: OperationErrors> Error<E> {
    /// Documentation for modeled service failures; unavailable for unknown identities
    /// and failures that did not produce a modeled service response.
    pub fn doc_url(&self) -> Option<&'static str> {
        match self {
            Self::Service(error) => error.doc_url(),
            Self::Unknown(error) => {
                E::classified_doc_url(error.metadata.status, error.type_name.as_deref())
            }
            _ => None,
        }
    }
}

impl<E> Error<E> {
    /// Converts the modeled error kind while preserving response metadata and
    /// every non-modeled failure. Generated service enums implement `From`
    /// for each operation's error enum.
    pub fn map_service<F>(self, map: impl FnOnce(E) -> F) -> Error<F> {
        match self {
            Self::Service(error) => Error::Service(ServiceError {
                error: map(error.error),
                metadata: error.metadata,
            }),
            Self::Unknown(error) => Error::Unknown(error),
            Self::Transport(error) => Error::Transport(error),
            Self::Invalid(message) => Error::Invalid(message),
            Self::Decode(message) => Error::Decode(message),
            Self::Timeout => Error::Timeout,
            Self::Cancelled => Error::Cancelled,
        }
    }

    pub fn metadata(&self) -> Option<&ResponseMetadata> {
        match self {
            Self::Service(error) => Some(&error.metadata),
            Self::Unknown(error) => Some(&error.metadata),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownError {
    pub metadata: Box<ResponseMetadata>,
    pub type_name: Option<String>,
    pub message: Option<String>,
    /// Retained even when a known error's fields cannot be decoded.
    pub retryable: bool,
    pub throttling: bool,
}

impl<E: fmt::Debug> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Service(error) => write!(f, "{error:?}"),
            Self::Unknown(error) => write!(
                f,
                "HTTP {} {}: {}",
                error.metadata.status,
                error.type_name.as_deref().unwrap_or("error"),
                error.message.as_deref().unwrap_or("")
            ),
            Self::Transport(error) => write!(f, "transport: {error}"),
            Self::Invalid(message) => write!(f, "invalid request: {message}"),
            Self::Decode(message) => write!(f, "unreadable response: {message}"),
            Self::Timeout => f.write_str("call deadline exceeded"),
            Self::Cancelled => f.write_str("call cancelled"),
        }
    }
}

impl<E: fmt::Debug> std::error::Error for Error<E> {}

/// Sends operations for a generated client. Cheap to clone.
#[derive(Clone)]
pub struct Transport {
    config: Arc<Config>,
}

impl Transport {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    pub async fn send<O, E>(&self, input: &O::Input) -> Result<O::Output, Error<E>>
    where
        O: Operation,
        O::Output: DeserializeOwned,
        E: OperationErrors,
    {
        self.send_with_options::<O, E>(input, &CallOptions::default())
            .await
    }

    pub async fn send_with_options<O, E>(
        &self,
        input: &O::Input,
        options: &CallOptions,
    ) -> Result<O::Output, Error<E>>
    where
        O: Operation,
        O::Output: DeserializeOwned,
        E: OperationErrors,
    {
        self.send_described::<O, E>(input, options).await
    }

    async fn send_described<O, E>(
        &self,
        input: &O::Input,
        options: &CallOptions,
    ) -> Result<O::Output, Error<E>>
    where
        O: Operation,
        O::Output: DeserializeOwned,
        E: OperationErrors,
    {
        let deadline = options
            .timeout
            .or(self.config.timeout)
            .map(|timeout| {
                tokio::time::Instant::now()
                    .checked_add(timeout)
                    .ok_or_else(|| Error::Invalid("call timeout out of range".into()))
            })
            .transpose()?;
        let cancelled = async {
            match &options.cancellation {
                Some(token) => token.cancelled().await,
                None => std::future::pending().await,
            }
        };
        let expired = async {
            match deadline {
                // An expired budget must be ready even between timer ticks.
                Some(deadline) if deadline > tokio::time::Instant::now() => {
                    tokio::time::sleep_until(deadline).await;
                }
                Some(_) => (),
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            () = cancelled => Err(Error::Cancelled),
            () = expired => Err(Error::Timeout),
            result = self.send_inner::<O, E>(input) => result,
        }
    }

    async fn send_inner<O, E>(&self, input: &O::Input) -> Result<O::Output, Error<E>>
    where
        O: Operation,
        O::Output: DeserializeOwned,
        E: OperationErrors,
    {
        let info = O::INFO;
        let (url, mut bound_headers, body) = self.prepare_at(info, input, &self.config.endpoint)?;
        // The release this client was generated for (docs/api-versions-clients.md).
        if let Some(version) = O::API_VERSION
            && let Ok(value) = HeaderValue::from_str(version)
        {
            bound_headers.insert("crucible-version", value);
        }
        // A Content-Type bound to an input member outranks a payload's media
        // type, which outranks client header defaults.
        let modelled_content_type = bound_headers.contains_key(http::header::CONTENT_TYPE);
        let mut headers = self.config.headers.clone();
        headers.extend(bound_headers);
        let method = Method::from_bytes(info.method.as_bytes())
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let retry_safe = info.readonly || info.idempotent || info.idempotency_token.is_some();
        let retry = self.config.retry;
        let mut attempt = 1;
        loop {
            let request = self
                .request(
                    method.clone(),
                    &url,
                    &headers,
                    body.clone(),
                    modelled_content_type && Location::payload(info.input).is_some(),
                    crate::auth::OperationAuth::of::<O>(),
                )
                .await?;
            let mut response = match self.config.http.execute(request).await {
                Ok(response) => response,
                Err(error) => {
                    if retry_safe && attempt < retry.max_attempts {
                        tokio::time::sleep(backoff(attempt, &retry)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(Error::Transport(error));
                }
            };
            let status = response.status().as_u16();
            let response_headers = std::mem::take(response.headers_mut());
            let transient_status = retryable_status(status);
            let success = (200..300).contains(&status);
            // A connection lost mid-body is the same fault as one lost before
            // the response: retried under the same rule.
            let bytes = match response.bytes().await {
                Ok(bytes) => bytes,
                Err(error) => {
                    if retry_safe && attempt < retry.max_attempts {
                        tokio::time::sleep(backoff(attempt, &retry)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(Error::Transport(error));
                }
            };
            if success {
                return decode_output::<O, E>(info.output, &response_headers, &bytes);
            }
            let error = decode_error::<E>(status, &response_headers, &bytes);
            let (throttling, transient) = match &error {
                Error::Service(error) => {
                    let info = error.error.info();
                    (info.throttling, info.retryable || transient_status)
                }
                Error::Unknown(error) => (error.throttling, error.retryable || transient_status),
                _ => (false, transient_status),
            };
            if (throttling || (retry_safe && transient)) && attempt < retry.max_attempts {
                let delay =
                    retry_after(&response_headers).unwrap_or_else(|| backoff(attempt, &retry));
                tokio::time::sleep(delay).await;
                attempt += 1;
                continue;
            }
            return Err(error);
        }
    }

    /// URL, bound headers and body (with its content type), once: every
    /// attempt resends them.
    #[allow(clippy::type_complexity)]
    fn prepare_at<E, I: serde::Serialize>(
        &self,
        info: &'static crucible_wire::operation::OperationInfo,
        input: &I,
        endpoint: &str,
    ) -> Result<(String, HeaderMap, Option<Body>), Error<E>> {
        let mut body = Vec::with_capacity(128);
        let bound = split::write_bound(input, info.input, &mut body)
            .map_err(|error| Error::Invalid(error.to_string()))?;
        let mut url = endpoint.trim_end_matches('/').to_owned();
        for segment in info.path {
            url.push('/');
            match segment {
                Segment::Literal(text) => url.push_str(text),
                Segment::Label(name) | Segment::GreedyLabel(name) => {
                    let value = bound
                        .iter()
                        .find(|(member, _)| member.name == *name)
                        .map(|(member, value)| text(value, member.scalar))
                        .ok_or_else(|| Error::Invalid(format!("`{name}` is required")))?
                        .map_err(Error::Invalid)?;
                    if value.is_empty() {
                        return Err(Error::Invalid(format!("`{name}` is required")));
                    }
                    let greedy = matches!(segment, Segment::GreedyLabel(_));
                    if (greedy && value.split('/').any(|s| matches!(s, "." | "..")))
                        || matches!(value.as_str(), "." | "..")
                    {
                        return Err(Error::Invalid(format!(
                            "`{name}` cannot be a dot-only path segment"
                        )));
                    }
                    if greedy {
                        for (index, part) in value.split('/').enumerate() {
                            if index > 0 {
                                url.push('/');
                            }
                            encode(&mut url, part);
                        }
                    } else {
                        encode(&mut url, &value);
                    }
                }
                _ => return Err(Error::Invalid("unsupported path segment".into())),
            }
        }
        if info.path.is_empty() {
            url.push('/');
        }
        let mut separator = '?';
        for (name, value) in info.literal_query {
            url.push(separator);
            separator = '&';
            encode(&mut url, name);
            if let Some(value) = value {
                url.push('=');
                encode(&mut url, value);
            }
        }
        let mut headers = HeaderMap::new();
        for (member, value) in &bound {
            match member.location {
                Location::Query(param) => {
                    let items: Vec<String> = match (value, member.scalar) {
                        (Value::Array(items), Scalar::List(item)) => items
                            .iter()
                            .map(|value| text(value, *item))
                            .collect::<Result<_, _>>()
                            .map_err(Error::Invalid)?,
                        (Value::Array(items), _) => items.iter().map(split::text).collect(),
                        (other, scalar) => vec![text(other, scalar).map_err(Error::Invalid)?],
                    };
                    for item in items {
                        url.push(separator);
                        separator = '&';
                        encode(&mut url, param);
                        url.push('=');
                        encode(&mut url, &item);
                    }
                }
                Location::Header(name) => {
                    let name = HeaderName::from_bytes(name.as_bytes())
                        .map_err(|e| Error::Invalid(e.to_string()))?;
                    let value = HeaderValue::from_str(
                        &header_text(value, member.scalar).map_err(Error::Invalid)?,
                    )
                    .map_err(|e| Error::Invalid(e.to_string()))?;
                    headers.insert(name, value);
                }
                Location::Label
                | Location::Body
                | Location::Payload(_)
                | Location::TextPayload(_)
                | Location::DocumentPayload(_) => {}
                _ => return Err(Error::Invalid("unsupported member binding".into())),
            }
        }
        // A payload is sent raw under its media type; an absent one sends
        // no body.
        let body = match Location::payload(info.input) {
            Some((_, media_type)) => (!body.is_empty()).then(|| (media_type, Bytes::from(body))),
            None => info
                .input
                .iter()
                .any(|member| member.location == Location::Body)
                .then(|| (JSON, Bytes::from(body))),
        };
        Ok((url, headers, body))
    }

    async fn request<E>(
        &self,
        method: Method,
        url: &str,
        bound_headers: &HeaderMap,
        body: Option<Body>,
        modelled_content_type: bool,
        auth: crate::auth::OperationAuth,
    ) -> Result<reqwest::Request, Error<E>> {
        let url = reqwest::Url::parse(url).map_err(|error| Error::Invalid(error.to_string()))?;
        let mut request = reqwest::Request::new(method, url);
        let headers = request.headers_mut();
        headers.extend(self.config.headers.clone());
        headers.extend(bound_headers.clone());
        if let Some((content_type, body)) = body {
            let content_type = match content_type {
                JSON => HeaderValue::from_static(JSON),
                media_type => HeaderValue::from_str(media_type)
                    .map_err(|error| Error::Invalid(error.to_string()))?,
            };
            // Only a modelled Content-Type member overrides a payload's media
            // type; ordinary protocol documents always keep the media type
            // matching their encoding, whatever the configured defaults say.
            if !modelled_content_type {
                request
                    .headers_mut()
                    .insert(http::header::CONTENT_TYPE, content_type);
            }
            *request.body_mut() = Some(reqwest::Body::from(body));
        }
        crate::auth::authenticate(&self.config, &mut request, auth).await?;
        if self.config.content_sha256
            && (request.body().is_some()
                || matches!(
                    *request.method(),
                    Method::POST | Method::PUT | Method::PATCH
                ))
        {
            let bytes = request
                .body()
                .and_then(reqwest::Body::as_bytes)
                .unwrap_or_default();
            let digest = format!("{:x}", Sha256::digest(bytes));
            request.headers_mut().insert(
                "x-amz-content-sha256",
                HeaderValue::from_str(&digest).expect("hex digest is a valid header"),
            );
        }
        for interceptor in &self.config.interceptors {
            interceptor
                .intercept(&mut request)
                .map_err(Error::Invalid)?;
        }
        Ok(request)
    }
}

/// A request body: its content type and bytes.
type Body = (&'static str, Bytes);

const JSON: &str = "application/json";

fn decode_output<O, E>(
    members: &'static [MemberInfo],
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<O::Output, Error<E>>
where
    O: Operation,
    O::Output: DeserializeOwned,
{
    read_response(members, headers, body, || body.clone()).map_err(Error::Decode)
}

/// A response structure from its bound headers and its body: a JSON object,
/// or the payload member's bytes, text or JSON value.
fn read_response<T: DeserializeOwned>(
    members: &'static [MemberInfo],
    headers: &HeaderMap,
    body: &[u8],
    bytes: impl FnOnce() -> Bytes,
) -> Result<T, String> {
    let mut bound = header_members(members, headers)?;
    if let Some((member, _)) = Location::payload(members) {
        match member.location {
            Location::TextPayload(_) => {
                let text = std::str::from_utf8(body)
                    .map_err(|_| format!("{}: the payload is not UTF-8 text", member.name))?;
                bound.push((member.name, BoundValue::Text(Cow::Borrowed(text))));
            }
            // No body is an absent value.
            Location::DocumentPayload(_) if body.trim_ascii().is_empty() => {}
            Location::DocumentPayload(_) => bound.push((member.name, BoundValue::RawJson(body))),
            // The body is the payload, whatever it holds; it shares the
            // response's buffer.
            _ => bound.push((member.name, BoundValue::Bytes(bytes()))),
        }
        return bound::read_http(&[], bound, members).map_err(|error| error.to_string());
    }
    // An operation without output answers `null` or nothing; both read as `{}`.
    let body = if body.trim_ascii() == b"null" {
        &[]
    } else {
        body
    };
    bound::read_http(body, bound, members).map_err(|error| error.to_string())
}

/// Header members of a response, read as their kinds.
fn header_members<'h>(
    members: &[MemberInfo],
    headers: &'h HeaderMap,
) -> Result<Vec<(&'static str, BoundValue<'h>)>, String> {
    let mut bound = Vec::new();
    for member in members {
        if let Location::Header(name) = member.location {
            if matches!(member.scalar, Scalar::List(_)) {
                let mut fields = headers.get_all(name).iter().peekable();
                if fields.peek().is_none() {
                    continue;
                }
                // Repeated field lines contribute items just like comma-separated values.
                let mut items = Vec::new();
                for field in fields {
                    let text = field
                        .to_str()
                        .map_err(|_| format!("{name}: is not visible ASCII"))?;
                    let BoundValue::List(field_items) =
                        BoundValue::parse(Cow::Borrowed(text), member.scalar)
                            .map_err(|message| format!("{name}: {message}"))?
                    else {
                        unreachable!("list scalar parses as a list");
                    };
                    items.extend(field_items);
                }
                bound.push((member.name, BoundValue::List(items)));
            } else if let Some(value) = headers.get(name) {
                let text = value
                    .to_str()
                    .map_err(|_| format!("{name}: is not visible ASCII"))?;
                let value = BoundValue::parse(Cow::Borrowed(text), member.scalar)
                    .map_err(|message| format!("{name}: {message}"))?;
                bound.push((member.name, value));
            }
        }
    }
    Ok(bound)
}

/// For generated `OperationErrors::decode`: a modelled error from its body
/// and header members.
pub fn decode_modelled<T: DeserializeOwned>(
    info: &'static ErrorInfo,
    headers: &HeaderMap,
    body: &[u8],
) -> Option<T> {
    read_response(info.members, headers, body, || Bytes::copy_from_slice(body)).ok()
}

/// Configured error documentation distinguishes malformed bodies from omission.
/// Payload-bound errors consume raw bytes; only ordinary bodies require objects.
#[doc(hidden)]
pub fn decode_documented_modelled<T: DeserializeOwned>(
    info: &'static ErrorInfo,
    headers: &HeaderMap,
    body: &[u8],
) -> Option<T> {
    if Location::payload(info.members).is_none()
        && !body.is_empty()
        && (body.trim_ascii().first() != Some(&b'{')
            || serde_json::from_slice::<serde::de::IgnoredAny>(body).is_err())
    {
        return None;
    }
    decode_modelled(info, headers, body)
}

/// For generated `OperationErrors::decode`: the one error of `errors` with
/// this status, if exactly one has it.
pub fn by_status(status: u16, errors: &[&'static ErrorInfo]) -> Option<&'static str> {
    let mut matching = errors.iter().filter(|info| info.status == status);
    match (matching.next(), matching.next()) {
        (Some(only), None) => Some(only.name),
        _ => None,
    }
}

fn decode_error<E: OperationErrors>(status: u16, headers: &HeaderMap, body: &[u8]) -> Error<E> {
    let envelope: Option<Value> = serde_json::from_slice(body).ok();
    let header_name = headers
        .get("x-crucible-error")
        .and_then(|value| value.to_str().ok());
    let body_name = envelope
        .as_ref()
        .and_then(|e| e.get("__type").and_then(Value::as_str));
    // A qualified name (`namespace#Name:detail`) reduces to the shape name.
    let type_name = header_name.or(body_name).map(|name| {
        let name = name.split(':').next().unwrap_or(name);
        name.rsplit('#').next().unwrap_or(name).to_owned()
    });
    let type_name = type_name.as_deref();
    let info = E::classify(status, type_name);
    let metadata = Box::new(ResponseMetadata {
        status,
        headers: headers.clone(),
        request_id: ["x-request-id", "x-amzn-requestid", "x-amz-request-id"]
            .into_iter()
            .find_map(|name| headers.get(name)?.to_str().ok())
            .map(str::to_owned),
    });
    match E::decode(status, type_name, headers, body) {
        Some(error) => Error::Service(ServiceError { error, metadata }),
        None => Error::Unknown(UnknownError {
            metadata,
            type_name: type_name
                .or_else(|| info.map(|i| i.name))
                .map(str::to_owned),
            message: envelope
                .as_ref()
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            retryable: info.is_some_and(|i| i.retryable),
            throttling: info.is_some_and(|i| i.throttling),
        }),
    }
}

// Keep the fallback status policy aligned with every client runtime.
fn retryable_status(status: u16) -> bool {
    matches!(status, 500 | 502 | 503 | 504)
}

/// Full jitter: a uniform delay up to an exponentially growing cap.
fn backoff(attempt: u32, retry: &crate::config::Retry) -> Duration {
    let cap = retry
        .base_delay
        .saturating_mul(1u32 << (attempt - 1).min(16))
        .min(retry.max_delay);
    let fraction = f64::from(uuid::Uuid::new_v4().as_u128() as u32) / f64::from(u32::MAX);
    cap.mul_f64(fraction)
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    retry_after_at(headers, chrono::Utc::now())
}

fn retry_after_at(headers: &HeaderMap, now: chrono::DateTime<chrono::Utc>) -> Option<Duration> {
    let value = headers.get("retry-after")?.to_str().ok()?.trim();
    let delay = if let Ok(seconds) = value.parse::<f64>() {
        Duration::try_from_secs_f64(seconds).ok()?
    } else {
        let date = chrono::DateTime::parse_from_rfc2822(value).ok()?;
        date.signed_duration_since(now)
            .to_std()
            .unwrap_or(Duration::ZERO)
    };
    // Tokio's timer must also be able to represent the resulting deadline.
    std::time::Instant::now().checked_add(delay)?;
    Some(delay)
}

/// Percent-encodes everything but the unreserved characters.
pub(crate) fn encode(out: &mut String, text: &str) {
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Retry;
    use crucible_wire::operation::Fault;

    static A: ErrorInfo =
        crate::__generated::error_info("A", 400, Fault::Client, false, false, &[]);
    static B: ErrorInfo =
        crate::__generated::error_info("B", 404, Fault::Client, false, false, &[]);
    static C: ErrorInfo =
        crate::__generated::error_info("C", 400, Fault::Client, false, false, &[]);

    #[test]
    fn a_status_names_an_error_only_when_one_error_has_it() {
        assert_eq!(by_status(404, &[&A, &B, &C]), Some("B"));
        assert_eq!(by_status(400, &[&A, &B, &C]), None);
        assert_eq!(by_status(500, &[&A, &B]), None);
    }

    #[test]
    fn everything_but_unreserved_characters_is_percent_encoded() {
        let mut out = String::new();
        encode(&mut out, "a/b c~é");
        assert_eq!(out, "a%2Fb%20c~%C3%A9");
    }

    #[test]
    fn backoff_stays_under_its_growing_cap() {
        let retry = Retry {
            max_attempts: 10,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(250),
        };
        for _ in 0..100 {
            assert!(backoff(1, &retry) <= Duration::from_millis(100));
            assert!(backoff(5, &retry) <= Duration::from_millis(250));
        }
    }

    #[test]
    fn retry_after_reads_seconds_and_ignores_the_rest() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("1.5"));
        assert_eq!(retry_after(&headers), Some(Duration::from_millis(1500)));
        headers.insert("retry-after", HeaderValue::from_static("soon"));
        assert_eq!(retry_after(&headers), None);
        headers.insert("retry-after", HeaderValue::from_static("-1"));
        assert_eq!(retry_after(&headers), None);
    }

    #[test]
    fn retry_after_dates_zero_and_overflow_are_safe() {
        let now = chrono::DateTime::parse_from_rfc2822("Wed, 21 Oct 2015 07:28:00 GMT")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let mut headers = HeaderMap::new();
        for (value, expected) in [
            (
                "Wed, 21 Oct 2015 07:28:02 GMT",
                Some(Duration::from_secs(2)),
            ),
            ("Wed, 21 Oct 2015 07:27:59 GMT", Some(Duration::ZERO)),
            ("0", Some(Duration::ZERO)),
            ("1e300", None),
            ("NaN", None),
            ("inf", None),
            ("18446744073709551615", None),
        ] {
            headers.insert("retry-after", HeaderValue::from_str(value).unwrap());
            assert_eq!(retry_after_at(&headers, now), expected, "{value}");
        }
    }

    #[derive(Debug)]
    struct Malformed;
    static THROTTLED: ErrorInfo =
        crate::__generated::error_info("Throttled", 429, Fault::Client, true, true, &[]);
    impl OperationErrors for Malformed {
        fn decode(_: u16, _: Option<&str>, _: &HeaderMap, _: &[u8]) -> Option<Self> {
            None
        }
        fn info(&self) -> &'static ErrorInfo {
            &THROTTLED
        }
        fn classify(status: u16, name: Option<&str>) -> Option<&'static ErrorInfo> {
            match name.or_else(|| by_status(status, &[&THROTTLED]))? {
                "Throttled" => Some(&THROTTLED),
                _ => None,
            }
        }
    }

    #[test]
    fn malformed_fields_do_not_erase_the_error_name_or_retry_policy() {
        let headers = HeaderMap::new();
        for body in [
            br#"{"__type":"Throttled","message":42}"#.as_slice(),
            br#"{"message":42}"#.as_slice(),
        ] {
            let Error::Unknown(error) = decode_error::<Malformed>(429, &headers, body) else {
                panic!("unknown error")
            };
            assert_eq!(error.type_name.as_deref(), Some("Throttled"));
            assert!(error.retryable && error.throttling);
            assert_eq!(error.message, None);
        }
        let Error::Unknown(error) =
            decode_error::<Malformed>(429, &headers, br#"{"__type":"Future","message":42}"#)
        else {
            panic!("unknown error")
        };
        assert_eq!(error.type_name.as_deref(), Some("Future"));
        assert!(!error.retryable && !error.throttling);
    }
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod expansion_tests;
