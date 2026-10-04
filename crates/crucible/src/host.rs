//! Hosting a service: a dedicated HTTP server (a container on ECS, a VM, a
//! laptop) or AWS Lambda. The service is the same either way; `run` picks the
//! host from the environment at startup, so moving between them is a
//! deployment change.
//!
//! ```ignore
//! #[tokio::main]
//! async fn main() -> Result<(), crucible::host::BoxError> {
//!     crucible::host::run(service).await
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
    tracing::info!(address = %listener.local_addr()?, "crucible listening");
    axum::serve(listener, service.into_router())
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
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
        move |event: lambda_runtime::LambdaEvent<serde_json::Value>| {
            let service = service.clone();
            async move { lambda_event(&service, event.payload).await }
        },
    ))
    .await
}

/// One Lambda event, answered as its source expects: a direct invoke
/// (`{"operation": "GetCity", "input": {..}}`) gets `{"output": ..}` or
/// `{"error": ..}`; an HTTP event (Function URL, API Gateway, ALB) gets that
/// source's response shape. For tests, and for a runtime loop of your own.
#[cfg(feature = "lambda")]
pub async fn lambda_event(
    service: &Crucible,
    payload: serde_json::Value,
) -> Result<serde_json::Value, BoxError> {
    lambda_event::handle(service, payload).await
}

#[cfg(not(feature = "lambda"))]
pub async fn lambda(_: Crucible) -> Result<(), BoxError> {
    Err("crucible was built without the `lambda` feature".into())
}

#[cfg(feature = "lambda")]
mod lambda_event {
    use axum::body::Body;
    use axum::http::{HeaderMap, Request};
    use lambda_http::aws_lambda_events::alb::AlbTargetGroupResponse;
    use lambda_http::aws_lambda_events::apigw::{
        ApiGatewayProxyResponse, ApiGatewayV2httpResponse,
    };
    use lambda_http::request::{LambdaRequest, RequestOrigin};
    use serde_json::Value;

    use super::BoxError;
    use crate::service::Crucible;

    /// One Lambda event: a direct invoke (`{"operation": .., "input": ..}`)
    /// or an HTTP event, answered in the shape its source expects.
    pub(super) async fn handle(service: &Crucible, payload: Value) -> Result<Value, BoxError> {
        if let Some(operation) = invoked_operation(&payload) {
            let input = payload
                .get("input")
                .cloned()
                .unwrap_or(Value::Object(Default::default()));
            return Ok(service.invoke(&operation, input).await);
        }
        let request: LambdaRequest = serde_json::from_value(payload)?;
        let origin = request.request_origin();
        let (parts, body) = axum::http::Request::<lambda_http::Body>::from(request).into_parts();
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
        respond(
            origin,
            status,
            parts.headers,
            body,
            base64,
            parts.status.canonical_reason(),
        )
    }

    /// A direct invoke names an operation and carries no HTTP envelope.
    fn invoked_operation(payload: &Value) -> Option<String> {
        let object = payload.as_object()?;
        if object.contains_key("requestContext") || object.contains_key("httpMethod") {
            return None;
        }
        object.get("operation")?.as_str().map(str::to_owned)
    }

    fn respond(
        origin: RequestOrigin,
        status: i64,
        headers: HeaderMap,
        body: Option<lambda_http::Body>,
        base64: bool,
        reason: Option<&str>,
    ) -> Result<Value, BoxError> {
        Ok(match origin {
            RequestOrigin::ApiGatewayV1 => {
                let mut response = ApiGatewayProxyResponse::default();
                response.status_code = status;
                response.body = body;
                response.is_base64_encoded = base64;
                // API Gateway v1 merges both header fields, so only one is set.
                response.multi_value_headers = headers;
                serde_json::to_value(response)?
            }
            RequestOrigin::ApiGatewayV2 => {
                let mut response = ApiGatewayV2httpResponse::default();
                response.status_code = status;
                response.body = body;
                response.is_base64_encoded = base64;
                response.headers = headers;
                serde_json::to_value(response)?
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
                serde_json::to_value(response)?
            }
            _ => return Err("unsupported Lambda event source".into()),
        })
    }
}
