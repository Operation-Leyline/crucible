//! Hosting a service: a dedicated HTTP server (a container on ECS, a VM, a
//! laptop) or AWS Lambda. The service is the same either way; `run` picks the
//! host from the environment at startup, so moving between them is a
//! deployment change.
//!
//! ```ignore
//! #[tokio::main]
//! async fn main() -> Result<(), crucible_server_runtime_rust::host::BoxError> {
//!     crucible_server_runtime_rust::host::run(service).await
//! }
//! ```

use std::net::SocketAddr;

use crate::service::Crucible;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Where a service runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    /// Listen for HTTP on this address.
    Http(SocketAddr),
    /// Take events from the Lambda runtime: HTTP events (Function URLs, API
    /// Gateway, ALB) and direct JSON invokes alike.
    Lambda,
}

impl Host {
    /// Lambda when the Lambda runtime started the process
    /// (`AWS_LAMBDA_RUNTIME_API` is set); otherwise HTTP on `CRUCIBLE_ADDR`,
    /// else `0.0.0.0:$PORT`, else `0.0.0.0:8080`.
    pub fn from_env() -> Result<Self, BoxError> {
        if std::env::var_os("AWS_LAMBDA_RUNTIME_API").is_some() {
            return Ok(Self::Lambda);
        }
        let address = match (std::env::var("CRUCIBLE_ADDR"), std::env::var("PORT")) {
            (Ok(address), _) => address,
            (Err(_), Ok(port)) => format!("0.0.0.0:{port}"),
            _ => "0.0.0.0:8080".to_owned(),
        };
        Ok(Self::Http(address.parse()?))
    }
}

/// Runs `service` on the host the environment names (see [`Host::from_env`]).
pub async fn run(service: Crucible) -> Result<(), BoxError> {
    serve(service, Host::from_env()?).await
}

pub async fn serve(service: Crucible, host: Host) -> Result<(), BoxError> {
    match host {
        Host::Http(address) => http(service, address).await,
        Host::Lambda => lambda(service).await,
    }
}

/// Serves HTTP until SIGTERM or Ctrl-C, then finishes the requests in flight:
/// what a container orchestrator expects when it replaces a task.
#[cfg(feature = "http")]
pub async fn http(service: Crucible, address: SocketAddr) -> Result<(), BoxError> {
    http_on(service, tokio::net::TcpListener::bind(address).await?).await
}

/// [`http`] on a listener the caller bound: to learn the port of `:0`, or to
/// take a socket from a supervisor.
#[cfg(feature = "http")]
pub async fn http_on(service: Crucible, listener: tokio::net::TcpListener) -> Result<(), BoxError> {
    http_on_until(service, listener, shutdown()).await
}

/// [`http_on`] until `signal` completes instead of SIGTERM or Ctrl-C: it then
/// stops accepting, lets each connection finish its request in flight, and
/// returns once they have closed.
///
/// The service is hyper's directly: Crucible routes every request itself, so
/// a router in front would only add work. HTTP/1.1 with upgrades, as before.
#[cfg(feature = "http")]
pub async fn http_on_until(
    service: Crucible,
    listener: tokio::net::TcpListener,
    signal: impl Future<Output = ()> + Send,
) -> Result<(), BoxError> {
    tracing::info!(address = %listener.local_addr()?, "crucible listening");
    http_server::serve(service, listener, signal).await;
    Ok(())
}

#[cfg(feature = "http")]
mod http_server {
    use std::convert::Infallible;
    use std::net::SocketAddr;
    use std::pin::{Pin, pin};
    use std::sync::Arc;
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::Request;
    use axum::response::Response;
    use hyper::body::Incoming;
    use hyper_util::rt::{TokioIo, TokioTimer};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::watch;

    use crate::service::Crucible;

    /// One `Arc` clone per request; the service itself is cloned per
    /// connection only.
    #[derive(Clone)]
    struct Handler(Arc<Crucible>, Option<SocketAddr>);

    impl hyper::service::Service<Request<Incoming>> for Handler {
        type Response = Response;
        type Error = Infallible;
        type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

        fn call(&self, mut request: Request<Incoming>) -> Self::Future {
            if let Some(peer) = self.1 {
                request.extensions_mut().insert(peer);
            }
            let service = self.0.clone();
            Box::pin(async move { Ok(service.handle(request.map(Body::new)).await) })
        }
    }

    pub(super) async fn serve(
        service: Crucible,
        listener: TcpListener,
        signal: impl Future<Output = ()> + Send,
    ) {
        let handler = Handler(Arc::new(service), None);
        // Connections watch `stop` to start closing; the server waits on
        // `open` until every connection task has dropped its receiver.
        let (stop, _) = watch::channel(());
        let (open_tx, open) = watch::channel(());
        let mut signal = pin!(signal);
        loop {
            let stream = tokio::select! {
                stream = accept(&listener) => stream,
                () = &mut signal => break,
            };
            let mut handler = handler.clone();
            handler.1 = stream.peer_addr().ok();
            let mut stopping = stop.subscribe();
            let open = open.clone();
            tokio::spawn(async move {
                // The timer arms hyper's header read timeout (30 s by
                // default), so a client that never finishes its headers
                // cannot hold a connection open.
                let connection = hyper::server::conn::http1::Builder::new()
                    .timer(TokioTimer::new())
                    .serve_connection(TokioIo::new(stream), handler)
                    .with_upgrades();
                let mut connection = pin!(connection);
                let mut closing = false;
                loop {
                    tokio::select! {
                        result = connection.as_mut() => {
                            if let Err(error) = result {
                                tracing::trace!("connection ended: {error:#}");
                            }
                            break;
                        }
                        _ = stopping.changed(), if !closing => {
                            closing = true;
                            connection.as_mut().graceful_shutdown();
                        }
                    }
                }
                drop(open);
            });
        }
        drop(listener);
        stop.send_replace(());
        drop(open);
        open_tx.closed().await;
    }

    /// Accepts the next connection, riding out errors as `axum::serve` and
    /// hyper 0.14 did: a peer's reset is skipped, and anything else (running
    /// out of file descriptors, say) is logged and retried after a second.
    async fn accept(listener: &TcpListener) -> TcpStream {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => return stream,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::ConnectionReset
                    ) => {}
                Err(error) => {
                    tracing::error!("accept error: {error}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}

#[cfg(not(feature = "http"))]
pub async fn http(_: Crucible, _: SocketAddr) -> Result<(), BoxError> {
    Err("crucible was built without the `http` feature".into())
}

#[cfg(feature = "http")]
async fn shutdown() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = interrupt => {}
        () = terminate => {}
    }
}

#[cfg(feature = "lambda")]
pub async fn lambda(service: Crucible) -> Result<(), BoxError> {
    lambda_runtime::run(lambda_runtime::service_fn(
        move |event: lambda_runtime::LambdaEvent<Box<serde_json::value::RawValue>>| {
            let service = service.clone();
            let meta = invocation_meta(&event.context);
            async move { lambda_event_json_with(&service, event.payload.get(), meta).await }
        },
    ))
    .await
}

/// One Lambda event as raw JSON, answered as its source expects: a direct
/// invoke (`{"operation": "GetCity", "input": {..}}`) gets `{"output": ..}`
/// or `{"error": ..}`, its input read straight from the event text; an HTTP
/// event (Function URL, API Gateway, ALB) gets that source's response shape.
/// For a runtime loop of your own.
#[cfg(feature = "lambda")]
pub async fn lambda_event_json(
    service: &Crucible,
    payload: &str,
) -> Result<Box<serde_json::value::RawValue>, BoxError> {
    lambda_event_json_with(service, payload, crate::RequestMeta::default()).await
}

/// [`lambda_event_json`] with what the runtime knows about the invocation:
/// its id (used unless an HTTP event carries its own request id) and its
/// deadline, which extensions read from the context.
#[cfg(feature = "lambda")]
pub async fn lambda_event_json_with(
    service: &Crucible,
    payload: &str,
    meta: crate::RequestMeta,
) -> Result<Box<serde_json::value::RawValue>, BoxError> {
    lambda_event::handle(service, payload, meta).await
}

/// The invocation's id and deadline (milliseconds since the epoch) as the
/// context holds them.
#[cfg(feature = "lambda")]
fn invocation_meta(context: &lambda_runtime::Context) -> crate::RequestMeta {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let left = std::time::Duration::from_millis(context.deadline).saturating_sub(now);
    crate::RequestMeta {
        request_id: axum::http::HeaderValue::from_str(&context.request_id).ok(),
        deadline: (context.deadline > 0).then(|| std::time::Instant::now() + left),
    }
}

/// [`lambda_event_json`] with JSON values, for tests.
#[cfg(feature = "lambda")]
pub async fn lambda_event(
    service: &Crucible,
    payload: serde_json::Value,
) -> Result<serde_json::Value, BoxError> {
    let answer = lambda_event_json(service, &payload.to_string()).await?;
    Ok(serde_json::from_str(answer.get())?)
}

#[cfg(not(feature = "lambda"))]
pub async fn lambda(_: Crucible) -> Result<(), BoxError> {
    Err("crucible was built without the `lambda` feature".into())
}

#[cfg(feature = "lambda")]
mod lambda_event {
    use std::borrow::Cow;

    use axum::body::{Body, Bytes};
    use axum::http::{HeaderMap, HeaderValue, Request, header::SET_COOKIE};
    use lambda_http::RequestExt;
    use lambda_http::aws_lambda_events::alb::AlbTargetGroupResponse;
    use lambda_http::aws_lambda_events::apigw::{
        ApiGatewayProxyResponse, ApiGatewayV2httpResponse,
    };
    use lambda_http::request::{LambdaRequest, RequestContext, RequestOrigin};
    use serde::Deserialize;
    use serde::de::IgnoredAny;
    use serde_json::value::{RawValue, to_raw_value};

    use super::BoxError;
    use crate::error::FrameworkError;
    use crate::service::Crucible;

    /// The most a synchronous invocation returns (Lambda quotas: 6 MB, in
    /// Lambda's MB of 1,048,576 bytes). The platform refuses a larger
    /// answer after the service has served and logged it, and the caller
    /// sees a bare 502 from the Function URL or CloudFront in front of it.
    const RESPONSE_LIMIT: usize = 6 * 1024 * 1024;

    /// Says why an answer was replaced, in the function's logs: the service
    /// log has already recorded the request as served.
    fn too_large(request_id: &str, size: usize) {
        tracing::error!(
            request_id,
            size,
            limit = RESPONSE_LIMIT,
            "the response is larger than a Lambda invocation returns; answered 500 instead"
        );
    }

    /// What tells a direct invoke from an HTTP event, read without building
    /// the event: the input stays a slice of the event text.
    #[derive(Deserialize)]
    struct Envelope<'a> {
        /// Raw: an HTTP event may carry an unrelated `operation` of any type,
        /// and only a string one makes a direct invoke.
        #[serde(borrow, default)]
        operation: Option<&'a RawValue>,
        #[serde(borrow, default)]
        input: Option<&'a RawValue>,
        #[serde(rename = "requestContext", default)]
        request_context: Option<IgnoredAny>,
        #[serde(rename = "httpMethod", default)]
        http_method: Option<IgnoredAny>,
    }

    /// The request id an API Gateway event carries in its request context.
    fn event_request_id(extensions: &axum::http::Extensions) -> Option<HeaderValue> {
        let id = match extensions.get::<RequestContext>()? {
            RequestContext::ApiGatewayV2(context) => context.request_id.as_deref(),
            RequestContext::ApiGatewayV1(context) => context.request_id.as_deref(),
            _ => None,
        };
        HeaderValue::from_str(id?).ok()
    }

    pub(super) async fn handle(
        service: &Crucible,
        payload: &str,
        meta: crate::RequestMeta,
    ) -> Result<Box<RawValue>, BoxError> {
        let envelope: Envelope<'_> = serde_json::from_str(payload)?;
        let operation = envelope
            .operation
            .and_then(|raw| serde_json::from_str::<Cow<'_, str>>(raw.get()).ok());
        if let (Some(operation), None, None) =
            (operation, &envelope.request_context, &envelope.http_method)
        {
            let input = envelope.input.map_or("{}", RawValue::get);
            let meta_id = meta.request_id.clone();
            let answer = service
                .invoke_json_with(&operation, Bytes::copy_from_slice(input.as_bytes()), meta)
                .await;
            if answer.len() > RESPONSE_LIMIT {
                too_large(&request_id(meta_id.as_ref()), answer.len());
                return Ok(
                    RawValue::from_string(crate::service::INVOKE_FAILURE.to_owned())
                        .expect("the failure envelope is JSON"),
                );
            }
            // A custom protocol may answer bytes that are not JSON.
            let answer = String::from_utf8(answer.to_vec())
                .ok()
                .and_then(|text| RawValue::from_string(text).ok());
            return Ok(answer.unwrap_or_else(|| {
                RawValue::from_string(crate::service::INVOKE_FAILURE.to_owned())
                    .expect("the failure envelope is JSON")
            }));
        }
        let mut request: LambdaRequest = serde_json::from_str(payload)?;
        // A Function URL or HTTP API passes the query as the client sent it;
        // lambda_http appends it to the URI and panics if the URI then fails
        // to parse (`"`, `<`, `>`, a space, from curl and the like).
        if let LambdaRequest::ApiGatewayV2(event) = &mut request
            && let Some(query) = &mut event.raw_query_string
        {
            *query = uri_query(query);
        }
        let origin = request.request_origin();
        // The event's own request id, as `event_request_id` reads it after
        // the conversion, for an answer when the conversion fails.
        let event_id = match &request {
            LambdaRequest::ApiGatewayV2(event) => event.request_context.request_id.as_deref(),
            LambdaRequest::ApiGatewayV1(event) => event.request_context.request_id.as_deref(),
            _ => None,
        }
        .and_then(|id| HeaderValue::from_str(id).ok());
        // The conversion panics on what it cannot read (a body that claims
        // base64 but is not); that is the client's malformed request.
        let converted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            axum::http::Request::<lambda_http::Body>::from(request)
        }));
        let Ok(converted) = converted else {
            return malformed(origin, event_id.or(meta.request_id));
        };
        let (mut parts, body) = converted.into_parts();
        if matches!(
            origin,
            RequestOrigin::ApiGatewayV1 | RequestOrigin::ApiGatewayV2
        ) {
            // lambda_http adds the deployment stage to the URI; model routes
            // describe the original path, independent of that deployment.
            let path = parts.raw_http_path();
            let path = if path.is_empty() { "/" } else { path };
            let path_and_query = match parts.uri.query() {
                Some(query) => format!("{path}?{query}"),
                None => path.to_owned(),
            };
            let mut uri = parts.uri.clone().into_parts();
            uri.path_and_query = Some(path_and_query.parse()?);
            parts.uri = axum::http::Uri::from_parts(uri)?;
        }
        let event_id = event_request_id(&parts.extensions);
        parts.extensions.insert(crate::RequestMeta {
            request_id: event_id.or(meta.request_id),
            deadline: meta.deadline,
        });
        let body = match body {
            lambda_http::Body::Text(text) => text.into_bytes(),
            lambda_http::Body::Binary(bytes) => bytes,
            _ => Vec::new(),
        };
        let response = service
            .handle(Request::from_parts(parts, Body::from(body)))
            .await;
        let (parts, body) = response.into_parts();
        let bytes = axum::body::to_bytes(body, usize::MAX).await?.to_vec();
        let (body, base64) = match String::from_utf8(bytes) {
            Ok(text) if text.is_empty() => (None, false),
            Ok(text) => (Some(lambda_http::Body::Text(text)), false),
            Err(error) => (Some(lambda_http::Body::Binary(error.into_bytes())), true),
        };
        let status = i64::from(parts.status.as_u16());
        let id = parts.headers.get("x-request-id").cloned();
        let answer = respond(
            origin.clone(),
            status,
            parts.headers,
            body,
            base64,
            parts.status.canonical_reason(),
        )?;
        if answer.get().len() <= RESPONSE_LIMIT {
            return Ok(answer);
        }
        // The caller gets the framework's internal failure with the request
        // id to quote, in the event's shape, instead of the platform's 502.
        too_large(&request_id(id.as_ref()), answer.get().len());
        let failure = FrameworkError::InternalFailure;
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        if let Some(id) = id {
            headers.insert("x-request-id", id);
        }
        let body = serde_json::json!({ "__type": failure.name(), "message": failure.message() });
        respond(
            origin,
            i64::from(failure.status()),
            headers,
            Some(lambda_http::Body::Text(body.to_string())),
            false,
            Some("Internal Server Error"),
        )
    }

    /// `query` with every byte a URI query does not carry raw
    /// percent-encoded, so it parses. Existing `%` escapes are kept, and a
    /// malformed one still reaches the protocol, which rejects it as the
    /// client's. Decoding gives back exactly what the client sent.
    fn uri_query(query: &str) -> String {
        let mut out = String::with_capacity(query.len());
        for byte in query.bytes() {
            if byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/?%".contains(&byte) {
                out.push(char::from(byte));
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        out
    }

    /// A 400 in the event's shape for an event lambda_http cannot convert.
    fn malformed(
        origin: RequestOrigin,
        request_id: Option<HeaderValue>,
    ) -> Result<Box<RawValue>, BoxError> {
        tracing::warn!(
            request_id = request_id.as_ref().and_then(|id| id.to_str().ok()),
            "the Lambda event is not a request that can be read; answered 400"
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        if let Some(id) = request_id {
            headers.insert("x-request-id", id);
        }
        let body = serde_json::json!({ "message": "the request could not be read" });
        respond(
            origin,
            400,
            headers,
            Some(lambda_http::Body::Text(body.to_string())),
            false,
            Some("Bad Request"),
        )
    }

    fn request_id(id: Option<&HeaderValue>) -> String {
        id.and_then(|id| id.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    }

    fn respond(
        origin: RequestOrigin,
        status: i64,
        headers: HeaderMap,
        body: Option<lambda_http::Body>,
        base64: bool,
        reason: Option<&str>,
    ) -> Result<Box<RawValue>, BoxError> {
        Ok(match origin {
            RequestOrigin::ApiGatewayV1 => {
                let mut response = ApiGatewayProxyResponse::default();
                response.status_code = status;
                response.body = body;
                response.is_base64_encoded = base64;
                // API Gateway v1 merges both header fields, so only one is set.
                response.multi_value_headers = headers;
                to_raw_value(&response)?
            }
            RequestOrigin::ApiGatewayV2 => {
                let mut response = ApiGatewayV2httpResponse::default();
                response.status_code = status;
                response.body = body;
                response.is_base64_encoded = base64;
                let mut headers = headers;
                // HTTP API v2 carries each Set-Cookie value in its cookies array.
                response.cookies = headers
                    .get_all(SET_COOKIE)
                    .iter()
                    .map(|value| std::str::from_utf8(value.as_bytes()).map(str::to_owned))
                    .collect::<Result<Vec<_>, _>>()?;
                headers.remove(SET_COOKIE);
                // HTTP API v2 has one string per header name. Joining bytes
                // preserves valid non-ASCII values that HeaderValue::to_str rejects.
                for name in headers.keys() {
                    let mut values = headers.get_all(name).iter();
                    let first = values.next().expect("header key has a value");
                    let value = if let Some(second) = values.next() {
                        let mut joined = first.as_bytes().to_vec();
                        for value in std::iter::once(second).chain(values) {
                            joined.push(b',');
                            joined.extend_from_slice(value.as_bytes());
                        }
                        HeaderValue::from_bytes(&joined).expect("joined header values are valid")
                    } else {
                        first.clone()
                    };
                    response.headers.insert(name.clone(), value);
                }
                to_raw_value(&response)?
            }
            RequestOrigin::Alb => {
                let mut response = AlbTargetGroupResponse::default();
                response.status_code = status;
                response.body = body;
                response.is_base64_encoded = base64;
                response.status_description =
                    Some(format!("{status} {}", reason.unwrap_or_default()));
                // An ALB reads one field or the other, depending on its configuration.
                response.headers = headers.clone();
                response.multi_value_headers = headers;
                to_raw_value(&response)?
            }
            _ => return Err("unsupported Lambda event source".into()),
        })
    }
}
