use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use super::{ToolDescriptor, ToolRegistry};
use crate::endpoint::EndpointAdmission;
use crate::protocol::{Answers, Protocol, ProtocolMatch, Route};
use crate::{FrameworkError, Hook, Job, OperationInfo, Operations, Prepare, Resolution, Result};

const VERSIONS: &[&str] = &["2025-03-26", "2025-06-18", "2025-11-25"];

#[derive(Clone)]
struct Call {
    id: Value,
    tool: String,
    arguments: Value,
}

/// Stateless Streamable HTTP: one JSON response per request, no SSE or sessions.
/// Use `Builder::mcp` to register this before ordinary protocols.
pub(crate) struct McpProtocol {
    descriptors: Option<&'static [ToolDescriptor]>,
    registry: Option<ToolRegistry>,
    cors: Option<crate::middlewares::Cors>,
    config: super::McpConfig,
    discovery: Option<super::oauth_discovery::OAuthDiscovery>,
    /// Well-known path and serialized document, built once in `prepare`.
    metadata: Option<(String, String)>,
}

impl McpProtocol {
    pub(crate) fn new(
        config: super::McpConfig,
        descriptors: Option<&'static [ToolDescriptor]>,
    ) -> Self {
        Self {
            descriptors,
            registry: None,
            cors: None,
            config,
            discovery: None,
            metadata: None,
        }
    }
    fn registry(&self) -> &ToolRegistry {
        self.registry.as_ref().expect("MCP prepared by builder")
    }

    async fn identify_request(
        &self,
        job: &mut Job,
        operations: &Operations,
        applies: &(dyn Fn(&OperationInfo) -> bool + Send + Sync),
    ) -> Result<ProtocolMatch> {
        if !self.owns(job.request()) {
            return Ok(ProtocolMatch::Declined);
        }
        job.read_body().await?;
        let message: Value = match serde_json::from_slice(job.body()) {
            Ok(value) => value,
            Err(_) => {
                job.respond(rpc_error(Value::Null, -32700, "Parse error"));
                return Ok(ProtocolMatch::Claimed(None));
            }
        };
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let valid_id = message
            .get("id")
            .is_none_or(|id| id.is_string() || id.as_i64().is_some() || id.as_u64().is_some());
        if !message.is_object()
            || message["jsonrpc"] != "2.0"
            || !message["method"].is_string()
            || !valid_id
        {
            job.respond(rpc_error(Value::Null, -32600, "Invalid Request"));
            return Ok(ProtocolMatch::Claimed(None));
        }
        // Valid notifications have no response, including unsupported methods.
        if message.get("id").is_none() {
            job.respond(StatusCode::ACCEPTED.into_response());
            return Ok(ProtocolMatch::Claimed(None));
        }
        let params = message.get("params");
        if params.is_some_and(|p| !p.is_object()) {
            job.respond(rpc_error(id, -32602, "Invalid params"));
            return Ok(ProtocolMatch::Claimed(None));
        }
        let params = params.cloned().unwrap_or_else(|| json!({}));
        let response = match message["method"].as_str().expect("checked") {
            "initialize" => {
                if !params["protocolVersion"].is_string()
                    || !params["capabilities"].is_object()
                    || !params["clientInfo"]["name"].is_string()
                    || !params["clientInfo"]["version"].is_string()
                {
                    rpc_error(id, -32602, "Invalid initialize params")
                } else {
                    let requested = params["protocolVersion"].as_str().expect("checked");
                    let version = if VERSIONS.contains(&requested) {
                        requested
                    } else {
                        VERSIONS[VERSIONS.len() - 1]
                    };
                    rpc_result(
                        id,
                        json!({"protocolVersion":version,"serverInfo":{"name":self.config.server_name,"version":self.config.server_version},"capabilities":{"tools":{"listChanged":false}}}),
                    )
                }
            }
            "ping" => rpc_result(id, json!({})),
            "tools/list" => {
                if params.get("cursor").is_some() {
                    rpc_error(id, -32602, "tools/list pagination is not supported")
                } else {
                    rpc_result(id, self.registry().list().clone())
                }
            }
            "tools/call" => {
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let descriptor = params["name"]
                    .as_str()
                    .and_then(|n| self.registry().descriptor(n));
                if let Some(descriptor) = descriptor
                    && arguments.is_object()
                    && let Some(entry) = operations.entry(descriptor.operation)
                    && applies(entry.info)
                {
                    job.context().set_attribute(Call {
                        id,
                        tool: descriptor.name.into(),
                        arguments,
                    });
                    return Ok(ProtocolMatch::Claimed(Some(Route {
                        operation: descriptor.operation,
                        labels: Vec::new(),
                        rejection: None,
                    })));
                }
                rpc_error(id, -32602, "Unknown tool or invalid arguments")
            }
            _ => rpc_error(id, -32601, "Method not found"),
        };
        job.respond(response);
        Ok(ProtocolMatch::Claimed(None))
    }
}

impl Protocol for McpProtocol {
    fn fallback(&self) -> bool {
        false
    }
    // Tools are described from the current model.
    fn current_version_only(&self, request: &Parts) -> bool {
        self.owns(request)
    }
    fn owns(&self, request: &Parts) -> bool {
        let path = request.uri.path();
        (path == self.config.path
            || self.discovery.as_ref().is_some_and(|d| path == d.path)
            || self.metadata.as_ref().is_some_and(|(p, _)| p == path))
            && request
                .extensions
                .get::<crate::protocol::Invocation>()
                .is_none()
    }
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        if !self.config.path.starts_with('/') || self.config.path.contains(['?', '#']) {
            return Err("MCP path must be an absolute URL path".into());
        }
        if self.config.server_name.is_empty() {
            self.config.server_name = prepare.service().id.to_owned();
        }
        if self.config.server_version.is_empty() {
            self.config.server_version = prepare.service().version.to_owned();
        }
        self.cors = prepare
            .service()
            .cors
            .map(|info| {
                crate::middlewares::Cors::new(info)
                    .and_then(|cors| cors.extend(&["POST"], &["mcp-protocol-version"]))
            })
            .transpose()?;
        if let Some(metadata) = &self.config.authorization_server {
            let path = metadata.well_known_path()?;
            if path == self.config.path {
                return Err("MCP path collides with the authorization server metadata path".into());
            }
            self.metadata = Some((path, metadata.document()));
        }
        let descriptors = self.descriptors.ok_or(
            "MCP descriptors were not generated; enable .mcp() in the service build script",
        )?;
        let registry = ToolRegistry::new(descriptors)?;
        if self.config.guard.is_some() && self.config.authorization.is_some() {
            return Err("MCP endpoint_guard and authorization cannot be combined".into());
        }
        self.discovery = self
            .config
            .authorization
            .as_ref()
            .map(|authorization| {
                super::oauth_discovery::OAuthDiscovery::new(
                    authorization.as_ref(),
                    self.config
                        .authorization_server
                        .as_ref()
                        .map(|m| m.issuer()),
                    &self.config.path,
                    descriptors,
                    prepare,
                )
            })
            .transpose()?;
        for descriptor in descriptors {
            if !prepare
                .operations()
                .any(|op| op.name == descriptor.operation && !op.internal)
            {
                return Err(format!(
                    "MCP tool {} targets an unavailable or internal operation",
                    descriptor.name
                ));
            }
        }
        self.registry = Some(registry);
        Ok(())
    }
    fn admit<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        Hook::wait(async move {
            if let Some(discovery) = &self.discovery
                && job.request().uri.path() == discovery.path
            {
                job.respond(if matches!(job.request().method.as_str(), "GET" | "HEAD") {
                    discovery.response()
                } else {
                    (
                        StatusCode::METHOD_NOT_ALLOWED,
                        [(header::ALLOW, "GET, HEAD")],
                    )
                        .into_response()
                });
                return Ok(());
            }
            // Authorization server metadata is public for the same reason.
            if let Some((path, document)) = &self.metadata
                && job.request().uri.path() == path
            {
                job.respond(metadata_response(&job.request().method, document));
                return Ok(());
            }
            // No Host-derived resource URLs or CORS-derived wildcard permission.
            let origins = job.request().headers.get_all(header::ORIGIN);
            if !self.config.any_origin
                && origins.iter().any(|origin| {
                    !origin
                        .to_str()
                        .ok()
                        .is_some_and(|o| self.config.origins.iter().any(|allowed| allowed == o))
                })
            {
                job.respond(StatusCode::FORBIDDEN.into_response());
                return Ok(());
            }
            // Browsers send preflights without credentials; only CORS-approved
            // preflights may bypass endpoint authentication.
            if job.request().method == "OPTIONS"
                && let Some(cors) = &self.cors
            {
                crate::Middleware::before(cors, job).resolve().await?;
                if job.outcome().is_some() {
                    return Ok(());
                }
            }
            if let Some(guard) = &self.config.guard
                && let EndpointAdmission::Respond(response) =
                    guard.admit(job.request()).resolve().await
            {
                job.respond(response);
                return Ok(());
            }
            if let Some(authorization) = &self.config.authorization
                && let EndpointAdmission::Respond(response) =
                    authorization.admit(job.request()).resolve().await
            {
                job.respond(response);
                return Ok(());
            }
            if job.request().method != "POST" {
                job.respond(
                    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response(),
                );
                return Ok(());
            }
            let versions = job.request().headers.get_all("mcp-protocol-version");
            let version = job
                .request()
                .headers
                .get("mcp-protocol-version")
                .map_or(Some("2025-03-26"), |v| v.to_str().ok());
            if versions.iter().count() > 1 || !version.is_some_and(|v| VERSIONS.contains(&v)) {
                job.respond(StatusCode::BAD_REQUEST.into_response());
                return Ok(());
            }
            let content_type = job
                .request()
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok());
            if !content_type.is_some_and(|v| {
                v.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("application/json")
            }) {
                job.respond(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
            }
            Ok(())
        })
    }
    fn identify_scoped<'a>(
        &'a self,
        job: &'a mut Job,
        operations: &'a Operations,
        applies: &'a (dyn Fn(&OperationInfo) -> bool + Send + Sync),
    ) -> Hook<'a, Result<ProtocolMatch>> {
        Hook::wait(self.identify_request(job, operations, applies))
    }
    fn decode<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        Hook::Ready((|| {
            let call = job
                .context()
                .attribute::<Call>()
                .ok_or_else(|| crate::Error::internal("missing MCP call"))?;
            let arguments = self
                .registry()
                .arguments(&call.tool, call.arguments)
                .map_err(|message| crate::validate::Rejection::malformed("", message))?;
            let input = job.decode(arguments)?;
            job.set_input(input);
            Ok(())
        })())
    }
    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response {
        if !self.owns(job.request()) {
            return StatusCode::NOT_FOUND.into_response();
        }
        // Authenticator chains can substitute modeled errors while retaining
        // the typed refusal on Context. Authentication stays an HTTP refusal,
        // with the ordinary error body and challenge, rather than a tool error.
        let refusal = match job.resolution() {
            Some(Resolution::Framework(error)) => Some(error),
            Some(Resolution::Modelled(_) | Resolution::Internal(_)) => {
                job.context().attribute::<FrameworkError>()
            }
            _ => None,
        };
        if matches!(
            refusal,
            Some(FrameworkError::Unauthenticated | FrameworkError::Forbidden)
        ) {
            return crate::protocol::JsonProtocol.write(job, answers);
        }
        let Some(call) = job.context().attribute::<Call>() else {
            // Failures before tool routing retain their HTTP status and type.
            return crate::protocol::JsonProtocol.write(job, answers);
        };
        match job.resolution() {
            Some(Resolution::Success(output)) => {
                let mut bytes = Vec::new();
                let serialized = output
                    .write_json(&mut bytes)
                    .map_err(|e| e.to_string())
                    .and_then(|_| serde_json::from_slice(&bytes).map_err(|e| e.to_string()));
                let checked = serialized.and_then(|v| {
                    let value = if job
                        .context()
                        .operation_metadata()
                        .is_some_and(|m| !m.has_output)
                    {
                        json!({})
                    } else {
                        v
                    };
                    self.registry().output(&call.tool, value)
                });
                match checked {
                    Ok(output) => rpc_result(
                        call.id,
                        json!({"structuredContent":output,"content":[{"type":"text","text":output.to_string()}],"isError":false}),
                    ),
                    Err(error) => {
                        let mut response =
                            rpc_error(call.id, -32603, "Output does not conform to outputSchema");
                        response
                            .extensions_mut()
                            .insert(crate::protocol::SerializationFailure(error));
                        response
                    }
                }
            }
            Some(Resolution::Modelled(error)) => {
                let descriptor = self
                    .registry()
                    .descriptor(&call.tool)
                    .expect("selected tool");
                let Some(external) = descriptor
                    .errors
                    .iter()
                    .find(|e| e.name == error.info().name)
                else {
                    return rpc_error(call.id, -32603, "Internal error");
                };
                let fields = error.to_json();
                let message = external
                    .message_member
                    .and_then(|m| fields.get(m))
                    .and_then(Value::as_str)
                    .unwrap_or("operation failed");
                rpc_result(
                    call.id,
                    json!({"content":[{"type":"text","text":json!({"name":external.name,"message":message}).to_string()}],"isError":true}),
                )
            }
            Some(Resolution::Rejected(rejection)) => {
                rpc_error(call.id, -32602, &rejection.summary())
            }
            // A conflict is the caller's to resolve (a reused idempotency
            // token), so the agent sees it as a tool error it can act on.
            Some(Resolution::Framework(error @ FrameworkError::Conflict { .. })) => rpc_result(
                call.id,
                json!({"content":[{"type":"text","text":json!({"name":error.name(),"message":error.message()}).to_string()}],"isError":true}),
            ),
            _ => rpc_error(call.id, -32603, "Internal error"),
        }
    }
    fn decorate_response(&self, response: &mut Response) {
        if let Some(discovery) = &self.discovery {
            discovery.decorate(response);
        }
    }
}

fn metadata_response(method: &axum::http::Method, document: &str) -> Response {
    if !matches!(method.as_str(), "GET" | "HEAD") {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, "GET, HEAD")],
        )
            .into_response();
    }
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
            // A public document with no credentials; browser clients may read it.
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
        ],
        document.to_owned(),
    )
        .into_response()
}
fn rpc_result(id: Value, result: Value) -> Response {
    json_response(json!({"jsonrpc":"2.0","id":id,"result":result}))
}
fn rpc_error(id: Value, code: i32, message: &str) -> Response {
    json_response(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}))
}
fn json_response(value: Value) -> Response {
    (
        [(header::CONTENT_TYPE, "application/json")],
        value.to_string(),
    )
        .into_response()
}
