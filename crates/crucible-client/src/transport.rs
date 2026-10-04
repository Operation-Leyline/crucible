//! Sending an operation: the request built from the operation's
//! description, attempts retried by the rules in `docs/framework.md`, and
//! the response decoded into the output or the operation's error.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use crucible_wire::bound::{self, BoundValue};
use crucible_wire::operation::{ErrorInfo, Location, MemberInfo, Operation, Segment};
use crucible_wire::split;
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::config::Config;

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
}

/// Why a call failed.
#[derive(Debug)]
pub enum Error<E> {
    /// A modelled error the operation declares.
    Service(E),
    /// An error response the client cannot name: an unmodelled one, or a
    /// newer service's.
    Unknown(UnknownError),
    /// No response: connection, TLS, timeout.
    Transport(reqwest::Error),
    /// The request could not be built from the input.
    Invalid(String),
    /// A response the client could not read as the model says.
    Decode(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownError {
    pub status: u16,
    pub type_name: Option<String>,
    pub message: Option<String>,
    pub request_id: Option<String>,
}

impl<E: fmt::Debug> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Service(error) => write!(f, "{error:?}"),
            Self::Unknown(error) => write!(
                f,
                "HTTP {} {}: {}",
                error.status,
                error.type_name.as_deref().unwrap_or("error"),
                error.message.as_deref().unwrap_or("")
            ),
            Self::Transport(error) => write!(f, "transport: {error}"),
            Self::Invalid(message) => write!(f, "invalid request: {message}"),
            Self::Decode(message) => write!(f, "unreadable response: {message}"),
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
        let info = O::INFO;
        let (url, headers, body) = self.prepare(info, input)?;
        let method = Method::from_bytes(info.method.as_bytes())
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let retry_safe = info.readonly || info.idempotent || info.idempotency_token.is_some();
        let retry = self.config.retry;
        let mut attempt = 1;
        loop {
            let request = self
                .request(method.clone(), &url, &headers, body.clone())
                .await?;
            let response = match self.config.http.execute(request).await {
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
            let response_headers = response.headers().clone();
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
            if (200..300).contains(&status) {
                return decode_output::<O, E>(info.output, &response_headers, &bytes);
            }
            let error = decode_error::<E>(status, &response_headers, &bytes);
            let (throttling, transient) = match &error {
                Error::Service(error) => {
                    let info = error.info();
                    (info.throttling, info.retryable || retryable_status(status))
                }
                _ => (false, retryable_status(status)),
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

    /// URL, bound headers and body, once: every attempt resends them.
    fn prepare<E, I: serde::Serialize>(
        &self,
        info: &'static crucible_wire::operation::OperationInfo,
        input: &I,
    ) -> Result<(String, HeaderMap, Option<Bytes>), Error<E>> {
        let mut body = Vec::with_capacity(128);
        let bound = split::write_bound(input, info.input, &mut body)
            .map_err(|error| Error::Invalid(error.to_string()))?;
        let mut url = self.config.endpoint.clone();
        for segment in info.path {
            url.push('/');
            match segment {
                Segment::Literal(text) => url.push_str(text),
                Segment::Label(name) => {
                    let value = bound
                        .iter()
                        .find(|(member, _)| member.name == *name)
                        .map(|(_, value)| split::text(value))
                        .ok_or_else(|| Error::Invalid(format!("`{name}` is required")))?;
                    if value.is_empty() {
                        return Err(Error::Invalid(format!("`{name}` is required")));
                    }
                    encode(&mut url, &value);
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
                Location::Label | Location::Body => {}
            }
        }
        let has_body = info
            .input
            .iter()
            .any(|member| member.location == Location::Body);
        Ok((url, headers, has_body.then(|| Bytes::from(body))))
    }

    async fn request<E>(
        &self,
        method: Method,
        url: &str,
        bound_headers: &HeaderMap,
        body: Option<Bytes>,
    ) -> Result<reqwest::Request, Error<E>> {
        let url = reqwest::Url::parse(url).map_err(|error| Error::Invalid(error.to_string()))?;
        let mut request = reqwest::Request::new(method, url);
        let headers = request.headers_mut();
        headers.extend(self.config.headers.clone());
        headers.extend(bound_headers.clone());
        if let Some(body) = body {
            headers.insert(
                http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            *request.body_mut() = Some(reqwest::Body::from(body));
        }
        if let Some(token) = &self.config.token {
            if let Some(token) = token.get().await {
                let value = HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|error| Error::Invalid(error.to_string()))?;
                request
                    .headers_mut()
                    .insert(http::header::AUTHORIZATION, value);
            }
        }
        for interceptor in &self.config.interceptors {
            interceptor
                .intercept(&mut request)
                .map_err(Error::Invalid)?;
        }
        Ok(request)
    }
}

fn decode_output<O, E>(
    members: &'static [MemberInfo],
    headers: &HeaderMap,
    body: &[u8],
) -> Result<O::Output, Error<E>>
where
    O: Operation,
    O::Output: DeserializeOwned,
{
    let bound = header_members(members, headers).map_err(Error::Decode)?;
    // An operation without output answers `null` or nothing; both read as `{}`.
    let body = if body.trim_ascii() == b"null" {
        &[]
    } else {
        body
    };
    bound::read(body, bound).map_err(|error| Error::Decode(error.to_string()))
}

/// Header members of a response, read as their kinds.
fn header_members<'h>(
    members: &[MemberInfo],
    headers: &'h HeaderMap,
) -> Result<Vec<(&'static str, BoundValue<'h>)>, String> {
    let mut bound = Vec::new();
    for member in members {
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
    let bound = header_members(info.members, headers).ok()?;
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

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "__type")]
    type_name: Option<String>,
    message: Option<String>,
}

fn decode_error<E: OperationErrors>(status: u16, headers: &HeaderMap, body: &[u8]) -> Error<E> {
    let envelope: Option<Envelope> = serde_json::from_slice(body).ok();
    let type_name = envelope
        .as_ref()
        .and_then(|e| e.type_name.as_deref())
        .map(|name| name.rsplit('#').next().unwrap_or(name));
    match E::decode(status, type_name, headers, body) {
        Some(error) => Error::Service(error),
        None => Error::Unknown(UnknownError {
            status,
            type_name: type_name.map(str::to_owned),
            message: envelope.and_then(|e| e.message),
            request_id: headers
                .get("x-request-id")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
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
    let seconds: f64 = headers.get("retry-after")?.to_str().ok()?.parse().ok()?;
    (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds))
}

/// Percent-encodes everything but the unreserved characters.
fn encode(out: &mut String, text: &str) {
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
}
