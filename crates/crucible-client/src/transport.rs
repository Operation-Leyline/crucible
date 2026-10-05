//! Sending an operation: the request built from the operation's
//! description, attempts retried by the rules in `docs/framework.md`, and
//! the response decoded into the output or the operation's error.

use crucible_wire::stream::{ByteStream, ChunkStream};
use futures_util::StreamExt;
use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use crucible_wire::bound::{self, BoundValue};
use crucible_wire::operation::{ErrorInfo, Location, MemberInfo, Operation, Segment};
use crucible_wire::split;
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::config::{CallOptions, Config};
use sha2::{Digest, Sha256};

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

impl<E> Error<E> {
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
    pub(crate) fn call_timeout(&self) -> Option<Duration> {
        self.config.timeout
    }
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
        self.send_described::<O, E>(input, options, None).await
    }

    pub async fn send_with_metadata<O, E>(
        &self,
        input: &O::Input,
        options: &CallOptions,
        metadata: &crate::OperationMetadata,
    ) -> Result<O::Output, Error<E>>
    where
        O: Operation,
        O::Output: DeserializeOwned,
        E: OperationErrors,
    {
        self.send_described::<O, E>(input, options, Some(metadata))
            .await
    }

    async fn send_described<O, E>(
        &self,
        input: &O::Input,
        options: &CallOptions,
        metadata: Option<&crate::OperationMetadata>,
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
                Some(deadline) => tokio::time::sleep_until(deadline).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            () = cancelled => Err(Error::Cancelled),
            () = expired => Err(Error::Timeout),
            result = self.send_inner::<O, E>(input, deadline, options.cancellation.clone(), metadata) => result,
        }
    }

    async fn send_inner<O, E>(
        &self,
        input: &O::Input,
        deadline: Option<tokio::time::Instant>,
        cancellation: Option<crate::CancellationToken>,
        metadata: Option<&crate::OperationMetadata>,
    ) -> Result<O::Output, Error<E>>
    where
        O: Operation,
        O::Output: DeserializeOwned,
        E: OperationErrors,
    {
        let info = O::INFO;
        let endpoint = metadata
            .map(|metadata| metadata.endpoint(input, &self.config))
            .transpose()
            .map_err(Error::Invalid)?;
        let (url, bound_headers, body) = self.prepare_at(
            info,
            input,
            endpoint
                .as_ref()
                .map_or(&self.config.endpoint, |endpoint| &endpoint.url),
        )?;
        let mut headers = endpoint
            .as_ref()
            .map_or_else(HeaderMap::new, |endpoint| endpoint.headers.clone());
        headers.extend(self.config.headers.clone());
        headers.extend(bound_headers);
        let stream = O::input_stream(input).map(|stream| {
            match Location::payload(info.input).map(|(member, _)| member.location) {
                Some(Location::StreamingPayload {
                    minimum, maximum, ..
                }) => stream.with_bounds(minimum, maximum),
                _ => stream,
            }
        });
        if let Some(stream) = &stream {
            if let Some((member, media_type)) = Location::payload(info.input) {
                if matches!(
                    member.location,
                    Location::StreamingPayload {
                        requires_length: true,
                        ..
                    }
                ) && stream.length().is_none()
                {
                    return Err(Error::Invalid(
                        "streaming payload requires a known length".into(),
                    ));
                }
                headers.insert(
                    http::header::CONTENT_TYPE,
                    HeaderValue::from_str(media_type).map_err(|e| Error::Invalid(e.to_string()))?,
                );
            }
        }
        let replayable = stream.as_ref().is_none_or(ByteStream::replayable);
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
                    crate::auth::OperationAuth::of::<O>()
                        .with_properties(endpoint.as_ref().map(|endpoint| &endpoint.properties)),
                    stream.as_ref(),
                )
                .await?;
            let response = match self.config.http.execute(request).await {
                Ok(response) => response,
                Err(error) => {
                    if replayable && retry_safe && attempt < retry.max_attempts {
                        tokio::time::sleep(backoff(attempt, &retry)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(Error::Transport(error));
                }
            };
            let status = response.status().as_u16();
            let response_headers = response.headers().clone();
            if (200..300).contains(&status)
                && info
                    .output
                    .iter()
                    .any(|m| matches!(m.location, Location::StreamingPayload { .. }))
            {
                let mut body = ByteStream::new(response.bytes_stream());
                if let Some((member, _)) = Location::payload(info.output) {
                    if let Location::StreamingPayload {
                        minimum, maximum, ..
                    } = member.location
                    {
                        body = body.with_bounds(minimum, maximum);
                    }
                }
                let stream = guarded_stream(
                    body.open().map_err(|e| Error::Decode(e.to_string()))?,
                    deadline,
                    cancellation,
                );
                let mut bound = header_members(status, info.output, &response_headers)
                    .map_err(Error::Decode)?;
                if let Some((member, _)) = Location::payload(info.output) {
                    bound.push((member.name, BoundValue::Json(Value::Null)));
                }
                let mut output =
                    bound::read(&[], bound).map_err(|e| Error::Decode(e.to_string()))?;
                O::set_output_stream(&mut output, stream);
                return Ok(output);
            }
            // A connection lost mid-body is the same fault as one lost before
            // the response: retried under the same rule.
            let bytes = match response.bytes().await {
                Ok(bytes) => bytes,
                Err(error) => {
                    if replayable && retry_safe && attempt < retry.max_attempts {
                        tokio::time::sleep(backoff(attempt, &retry)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(Error::Transport(error));
                }
            };
            if (200..300).contains(&status) {
                return decode_output::<O, E>(status, info.output, &response_headers, &bytes);
            }
            let error = decode_error::<E>(status, &response_headers, &bytes);
            let (throttling, transient) = match &error {
                Error::Service(error) => {
                    let info = error.error.info();
                    (info.throttling, info.retryable || retryable_status(status))
                }
                Error::Unknown(error) => (
                    error.throttling,
                    error.retryable || retryable_status(status),
                ),
                _ => (false, retryable_status(status)),
            };
            if replayable
                && (throttling || (retry_safe && transient))
                && attempt < retry.max_attempts
            {
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
                        .map(|(_, value)| split::text(value))
                        .ok_or_else(|| Error::Invalid(format!("`{name}` is required")))?;
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
                    let items: Vec<String> = match value {
                        Value::Array(items) => items.iter().map(split::text).collect(),
                        other => vec![split::text(other)],
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
                    let value = HeaderValue::from_str(&split::text(value))
                        .map_err(|e| Error::Invalid(e.to_string()))?;
                    headers.insert(name, value);
                }
                Location::QueryParams => {
                    let entries = value.as_object().ok_or_else(|| {
                        Error::Invalid("query parameters must be a map".to_owned())
                    })?;
                    for (name, value) in entries {
                        if info.literal_query.iter().any(|(key, _)| key == name)
                            || info
                                .input
                                .iter()
                                .any(|m| matches!(m.location, Location::Query(key) if key == name))
                        {
                            continue;
                        }
                        let items = match value {
                            Value::Array(items) => items.as_slice(),
                            value => std::slice::from_ref(value),
                        };
                        for value in items {
                            url.push(separator);
                            separator = '&';
                            encode(&mut url, name);
                            url.push('=');
                            encode(&mut url, &split::text(value));
                        }
                    }
                }
                Location::PrefixHeaders(prefix) => {
                    let entries = value
                        .as_object()
                        .ok_or_else(|| Error::Invalid("prefix headers must be a map".to_owned()))?;
                    for (key, value) in entries {
                        let name = format!("{prefix}{key}");
                        if info.input.iter().any(|m| matches!(m.location, Location::Header(explicit) if explicit.eq_ignore_ascii_case(&name))) { continue; }
                        headers.insert(
                            HeaderName::from_bytes(name.as_bytes())
                                .map_err(|e| Error::Invalid(e.to_string()))?,
                            HeaderValue::from_str(&split::text(value))
                                .map_err(|e| Error::Invalid(e.to_string()))?,
                        );
                    }
                }
                Location::Label
                | Location::Body
                | Location::Payload(_)
                | Location::ResponseCode
                | Location::StreamingPayload { .. } => {}
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
        auth: crate::auth::OperationAuth<'_>,
        stream: Option<&ByteStream>,
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
            headers.insert(http::header::CONTENT_TYPE, content_type);
            *request.body_mut() = Some(reqwest::Body::from(body));
        }
        if let Some(stream) = stream {
            if self.config.content_sha256
                && stream.sha256().is_none()
                && !request.headers().contains_key("x-amz-content-sha256")
            {
                return Err(Error::Invalid(
                    "streaming content_sha256 requires a precomputed ByteStream sha256".into(),
                ));
            }
            if let Some(length) = stream.length() {
                request.headers_mut().insert(
                    http::header::CONTENT_LENGTH,
                    HeaderValue::from_str(&length.to_string()).expect("integer header"),
                );
            }
            if let Some(digest) = stream.sha256() {
                request.headers_mut().insert(
                    "x-amz-content-sha256",
                    HeaderValue::from_str(digest).expect("validated hex digest"),
                );
            }
            *request.body_mut() = Some(reqwest::Body::wrap_stream(
                stream.open().map_err(|e| Error::Invalid(e.to_string()))?,
            ));
        }
        let signing = crate::auth::authenticate(&self.config, &mut request, auth).await?;
        if self.config.content_sha256
            && stream.is_none()
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
        if let Some(signing) = signing {
            signing.apply(&mut request).map_err(Error::Invalid)?;
        }
        Ok(request)
    }
}

/// Dropping this body drops the HTTP reader; every read keeps the original call budget.
fn guarded_stream(
    stream: ChunkStream,
    deadline: Option<tokio::time::Instant>,
    cancellation: Option<crate::CancellationToken>,
) -> ByteStream {
    ByteStream::new(futures_util::stream::unfold(Some(stream), move |state| {
        let cancellation = cancellation.clone();
        async move {
            let mut stream = state?;
            let cancelled = async {
                match cancellation {
                    Some(token) => token.cancelled().await,
                    None => std::future::pending().await,
                }
            };
            let expired = async {
                match deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                biased;
                () = cancelled => Some((Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "call cancelled")), None)),
                () = expired => Some((Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "call deadline exceeded")), None)),
                next = stream.next() => next.map(|item| (item, Some(stream))),
            }
        }
    }))
}

/// A request body: its content type and bytes.
type Body = (&'static str, Bytes);

const JSON: &str = "application/json";

fn decode_output<O, E>(
    status: u16,
    members: &'static [MemberInfo],
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<O::Output, Error<E>>
where
    O: Operation,
    O::Output: DeserializeOwned,
{
    let mut bound = header_members(status, members, headers).map_err(Error::Decode)?;
    if let Some((member, _)) = Location::payload(members) {
        // The body is the payload, whatever it holds; it shares the
        // response's buffer.
        bound.push((member.name, BoundValue::Bytes(body.clone())));
        return bound::read(&[], bound).map_err(|error| Error::Decode(error.to_string()));
    }
    // An operation without output answers `null` or nothing; both read as `{}`.
    let body = if body.trim_ascii() == b"null" {
        &[]
    } else {
        &body[..]
    };
    bound::read(body, bound).map_err(|error| Error::Decode(error.to_string()))
}

/// Header members of a response, read as their kinds.
fn header_members<'h>(
    status: u16,
    members: &[MemberInfo],
    headers: &'h HeaderMap,
) -> Result<Vec<(&'static str, BoundValue<'h>)>, String> {
    let mut bound = Vec::new();
    for member in members {
        if member.location == Location::ResponseCode {
            bound.push((member.name, BoundValue::Integer(i64::from(status))));
        }
        if let Location::PrefixHeaders(prefix) = member.location {
            let mut values = serde_json::Map::new();
            for (name, value) in headers {
                if let Some(key) = name.as_str().strip_prefix(prefix) {
                    values.insert(
                        key.to_owned(),
                        Value::String(value.to_str().map_err(|e| e.to_string())?.to_owned()),
                    );
                }
            }
            bound.push((member.name, BoundValue::Json(Value::Object(values))));
        }
        if let Location::Header(name) = member.location {
            if let Some(value) = headers.get(name).and_then(|value| value.to_str().ok()) {
                let value = BoundValue::parse(Cow::Borrowed(value), member.scalar)
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
    decode_modelled_at(info.status, info, headers, body)
}

/// Decodes bound response members using the actual HTTP status.
pub fn decode_modelled_at<T: DeserializeOwned>(
    status: u16,
    info: &'static ErrorInfo,
    headers: &HeaderMap,
    body: &[u8],
) -> Option<T> {
    let bound = header_members(status, info.members, headers).ok()?;
    bound::read(body, bound).ok()
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
    let type_name = envelope
        .as_ref()
        .and_then(|e| e.get("__type"))
        .and_then(Value::as_str)
        .map(|name| name.rsplit('#').next().unwrap_or(name));
    let info = E::classify(status, type_name);
    let metadata = Box::new(ResponseMetadata {
        status,
        headers: headers.clone(),
        request_id: headers
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
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

    static A: ErrorInfo = ErrorInfo {
        name: "A",
        status: 400,
        fault: Fault::Client,
        retryable: false,
        throttling: false,
        members: &[],
    };
    static B: ErrorInfo = ErrorInfo {
        name: "B",
        status: 404,
        fault: Fault::Client,
        retryable: false,
        throttling: false,
        members: &[],
    };
    static C: ErrorInfo = ErrorInfo {
        name: "C",
        status: 400,
        fault: Fault::Client,
        retryable: false,
        throttling: false,
        members: &[],
    };

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
    static THROTTLED: ErrorInfo = ErrorInfo {
        name: "Throttled",
        status: 429,
        fault: Fault::Client,
        retryable: true,
        throttling: true,
        members: &[],
    };
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
