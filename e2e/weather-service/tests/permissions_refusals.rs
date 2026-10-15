//! A permission refusal: Hidden is the
//! modelled `NotFoundException` (404, never cached, naming only the
//! resource type), byte-identical to a handler's own `NotFoundException`
//! for a resource that is genuinely missing, and Denied a 403
//! `ForbiddenException` without an authentication challenge, whether the
//! request came over Crucible JSON, a direct invoke or an MCP tool call.
use axum::{body::Body, http::Request};
use crucible_server_runtime_rust::__generated as generated;
use crucible_server_runtime_rust::mcp::McpConfig;
use crucible_server_runtime_rust::permissions::{
    AuthorizationRequest, Authorizer, Check, Checks, Mode, PermissionInfo, Permissions,
    RequirementInfo, Requirements, ResourceInfo, ResourceRef, Tier, Verdict,
};
use crucible_server_runtime_rust::{
    Crucible, DecodedJob, FrameworkError, Hook, Interceptor, InterceptorExt, InvokeProtocol,
    ModelledError, Operation, Operations, Result,
    operation::{
        AuthScheme, ErrorInfo, Fault, Location, MemberInfo, OperationInfo, Redaction, Scalar,
        Segment, ServiceInfo, Unit,
    },
    protocol::Invocation,
};
use serde_json::{Value, json};
use std::sync::Arc;
use weather_service::MemoryCities;

const fn member(name: &'static str) -> MemberInfo {
    generated::member_info(
        name,
        Location::Body,
        Scalar::String,
        &Redaction::Public,
        &[],
    )
}

/// `crucible.errors#NotFoundException`, as a generated error describes it.
static NOT_FOUND: ErrorInfo = generated::error_info(
    "NotFoundException",
    404,
    Fault::Client,
    false,
    false,
    &[member("message"), member("resourceType")],
);

/// A handler's own `NotFoundException`, with a message of its choosing.
#[derive(Debug)]
struct Missing;

impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("no workspace w-secret")
    }
}

impl std::error::Error for Missing {}

impl ModelledError for Missing {
    fn info(&self) -> &'static ErrorInfo {
        &NOT_FOUND
    }
    fn shape_id(&self) -> &'static str {
        "crucible.errors#NotFoundException"
    }
    fn to_json(&self) -> Value {
        json!({ "message": "There is no workspace w-secret.", "resourceType": "workspace" })
    }
}

static INFO: OperationInfo = generated::operation_info(
    "example.auth#Probe",
    "Probe",
    None,
    "POST",
    &[Segment::Literal("probe")],
    &[],
    200,
    &[],
    &[],
    &["NotFoundException"],
    false,
    false,
    false,
    None,
    &[],
);

static WORKSPACE: ResourceInfo = ResourceInfo {
    name: "workspace",
    identifiers: &["workspaceId"],
    not_found: "No such workspace.",
};
static READ: PermissionInfo = PermissionInfo {
    resource: &WORKSPACE,
    symbol: "read",
    value: "workspace:read",
    aliases: &[],
    tier: Tier::Public,
    denied: "This principal does not hold workspace:read on that workspace.",
};

static SERVICE: ServiceInfo = generated::service_info(
    "example.auth#Refusals",
    "1",
    None,
    &[&INFO],
    &[&NOT_FOUND],
    |error| {
        error
            .downcast_ref::<Missing>()
            .map(|missing| missing as &dyn ModelledError)
    },
    None,
    None,
    None,
)
.with_permissions(&[&READ]);

/// `requires read on Workspace`, bound to a workspace the input does not
/// carry (it has none), so an empty request is enough. The
/// handler finds the workspace missing.
/// `OWN`: the service designates its own not-found error.
struct Probe<const OWN: bool>;
impl<const OWN: bool> Operation for Probe<OWN> {
    const INFO: &'static OperationInfo = if OWN { &OWN_INFO } else { &INFO };
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[AuthScheme::Bearer]);
    const HAS_INPUT: bool = false;
    const HAS_OUTPUT: bool = false;
    const REQUIREMENTS: Requirements = Requirements::List(&[RequirementInfo {
        mode: Mode::Check,
        permission: &READ,
        conditional: false,
    }]);
    const CHECKS: Option<Checks<Unit>> = Some(|_, out| {
        out.push(Check::declared(
            0,
            &READ,
            ResourceRef::new(&WORKSPACE, ["w-secret"]),
        ));
    });
    type Input = Unit;
    type Output = Unit;
}

struct Answering(Verdict);

#[crucible_server_runtime_rust::async_trait]
impl Authorizer for Answering {
    async fn decide(&self, _: &AuthorizationRequest<'_>, checks: &[Check]) -> Result<Vec<Verdict>> {
        Ok(vec![self.0; checks.len()])
    }
}

struct Answer {
    status: u16,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
    value: Value,
}

async fn call<const OWN: bool>(verdict: Verdict, invoke: bool) -> Answer {
    let operations = Operations::builder(if OWN { &OWN_SERVICE } else { &SERVICE })
        .add::<Probe<OWN>, _, _>(Arc::new(()), |_, _| async {
            Err::<Unit, _>(if OWN {
                own_missing(&ResourceRef::new(&WORKSPACE, ["w-secret"]))
            } else {
                Missing.into()
            })
        })
        .build()
        .unwrap();
    let mut permissions = Permissions::new(Answering(verdict));
    if OWN {
        permissions = permissions.not_found_factory(own_missing);
    }
    let service = Crucible::builder(operations)
        .defaults()
        .protocol(InvokeProtocol)
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .interceptor(permissions)
        .build()
        .unwrap();
    let mut request = Request::post("/probe")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    if invoke {
        request.extensions_mut().insert(Invocation {
            operation: "Probe".into(),
        });
    }
    let response = service.handle(request).await;
    let status = response.status().as_u16();
    let mut headers = response.headers().clone();
    // The request id is each request's own.
    headers.remove("x-request-id");
    headers.remove("x-amzn-requestid");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    let value: Value = serde_json::from_slice(&body).unwrap();
    let value = if invoke {
        value["error"].clone()
    } else {
        value
    };
    Answer {
        status,
        headers,
        body,
        value,
    }
}

async fn check(invoke: bool) {
    let context = format!("invoke {invoke}");

    let hidden = call::<false>(Verdict::Hidden, invoke).await;
    assert_eq!(hidden.status, 404, "{context}");
    assert_eq!(hidden.headers["cache-control"], "no-store", "{context}");
    assert!(
        !hidden.headers.contains_key("www-authenticate"),
        "{context}"
    );
    assert_eq!(
        hidden.value,
        json!({
            "__type": "NotFoundException",
            "message": "No such workspace.",
            "resourceType": "workspace",
        }),
        "{context}"
    );

    // Allowed, the handler answers its own NotFoundException for a missing
    // workspace: the same bytes and headers as the hidden one.
    let missing = call::<false>(Verdict::Allowed, invoke).await;
    assert_eq!(missing.status, hidden.status, "{context}");
    assert_eq!(missing.headers, hidden.headers, "{context}");
    assert_eq!(missing.body, hidden.body, "{context}");

    let denied = call::<false>(Verdict::Denied, invoke).await;
    assert_eq!(denied.status, 403, "{context}");
    assert!(
        !denied.headers.contains_key("www-authenticate"),
        "{context}"
    );
    assert_eq!(denied.value["__type"], "ForbiddenException", "{context}");
    assert_eq!(
        denied.value["message"], "This principal does not hold workspace:read on that workspace.",
        "{context}"
    );
    for answer in [&hidden, &denied] {
        let text = answer.value.to_string();
        assert!(!text.contains("w-secret"), "{context}: {text}");
    }
}

#[tokio::test]
async fn hidden_and_missing_answer_alike_and_denied_as_forbidden() {
    check(false).await;
    check(true).await;
}

// ---- a service's own not-found error ----------------------------------------

/// `example.auth#ResourceNotFoundException`: a service's own 404, with a
/// member the framework cannot fill, which the model designates
/// (`notFound` in the service) instead of `crucible.errors`.
static OWN_NOT_FOUND: ErrorInfo = generated::error_info(
    "ResourceNotFoundException",
    404,
    Fault::Client,
    false,
    false,
    &[
        member("message"),
        member("resourceType"),
        member("resourceId"),
    ],
);

#[derive(Debug)]
struct OwnMissing {
    resource_type: &'static str,
    resource_id: String,
}

impl std::fmt::Display for OwnMissing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no {} {}", self.resource_type, self.resource_id)
    }
}

impl std::error::Error for OwnMissing {}

impl ModelledError for OwnMissing {
    fn info(&self) -> &'static ErrorInfo {
        &OWN_NOT_FOUND
    }
    fn shape_id(&self) -> &'static str {
        "example.auth#ResourceNotFoundException"
    }
    fn to_json(&self) -> Value {
        json!({
            "message": "Not found.",
            "resourceType": self.resource_type,
            "resourceId": self.resource_id,
        })
    }
}

/// The service's one 404 constructor: its handler's for a missing
/// workspace, and its not-found factory's for a hidden one.
fn own_missing(resource: &ResourceRef) -> crucible_server_runtime_rust::Error {
    OwnMissing {
        resource_type: resource.resource().name,
        resource_id: resource
            .identifiers()
            .last()
            .map(ToString::to_string)
            .unwrap_or_default(),
    }
    .into()
}

static OWN_INFO: OperationInfo = generated::operation_info(
    "example.auth#Probe",
    "Probe",
    None,
    "POST",
    &[Segment::Literal("probe")],
    &[],
    200,
    &[],
    &[],
    &["ResourceNotFoundException"],
    false,
    false,
    false,
    None,
    &[],
);

static OWN_SERVICE: ServiceInfo = generated::service_info(
    "example.auth#Refusals",
    "1",
    None,
    &[&OWN_INFO],
    &[&OWN_NOT_FOUND],
    |error| {
        error
            .downcast_ref::<OwnMissing>()
            .map(|missing| missing as &dyn ModelledError)
    },
    None,
    None,
    None,
)
.with_permissions(&[&READ])
.with_not_found("example.auth#ResourceNotFoundException");

async fn check_own(invoke: bool) {
    let context = format!("invoke {invoke}");
    let hidden = call::<true>(Verdict::Hidden, invoke).await;
    assert_eq!(hidden.status, 404, "{context}");
    assert_eq!(hidden.headers["cache-control"], "no-store", "{context}");
    let expected = json!({
        "__type": "ResourceNotFoundException",
        "message": "Not found.",
        "resourceType": "workspace",
        "resourceId": "w-secret",
    });
    assert_eq!(hidden.value, expected, "{context}");
    // The handler's genuinely missing workspace: the same bytes and the
    // same headers, `Cache-Control: no-store` included.
    let missing = call::<true>(Verdict::Allowed, invoke).await;
    assert_eq!(missing.status, hidden.status, "{context}");
    assert_eq!(missing.headers, hidden.headers, "{context}");
    assert_eq!(missing.body, hidden.body, "{context}");
}

#[tokio::test]
async fn a_services_own_not_found_error_answers_hidden_and_missing_alike() {
    check_own(false).await;
    check_own(true).await;
}

/// Answers `Ping` with `error`, as an authorization or a handler would.
struct Refuse(FrameworkError);

impl Interceptor for Refuse {
    fn before<'a>(&'a self, _: &'a mut DecodedJob<'_>) -> Hook<'a, Result<()>> {
        Hook::Ready(Err(self.0.clone().into()))
    }
}

#[tokio::test]
async fn an_mcp_tool_call_gets_not_found_and_permission_denied_as_tool_errors() {
    for (error, cacheable) in [
        (
            FrameworkError::NotFound {
                resource: &WORKSPACE,
            },
            false,
        ),
        (FrameworkError::PermissionDenied { permission: &READ }, true),
    ] {
        let service = weather_service::builder(Arc::new(MemoryCities::default()))
            .mcp(McpConfig::new())
            .interceptor(Refuse(error.clone()).only(&["Ping"]))
            .build()
            .unwrap();
        let call = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": { "name": "Ping", "arguments": {} },
        });
        let response = service
            .handle(
                Request::post("/mcp")
                    .header("content-type", "application/json")
                    .header("accept", "application/json, text/event-stream")
                    .body(Body::from(call.to_string()))
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status().as_u16(), 200, "{error:?}");
        assert_eq!(
            response.headers().get("cache-control").is_none(),
            cacheable,
            "{error:?}"
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["result"]["isError"], true, "{body}");
        let text: Value =
            serde_json::from_str(body["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            text,
            json!({ "name": error.name(), "message": error.message() })
        );
    }
}
