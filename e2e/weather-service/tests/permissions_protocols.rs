//! A permission refusal reads the same in every protocol: Hidden is the
//! modelled `NotFoundException` (404, never cached, naming only the
//! resource type) and Denied a 403 `ForbiddenException` without an
//! authentication challenge, whether the request came over a JSON
//! protocol, RPC v2, a direct invoke or an MCP tool call.
use axum::{body::Body, http::Request};
use crucible_server_runtime_rust::__generated as generated;
use crucible_server_runtime_rust::mcp::McpConfig;
use crucible_server_runtime_rust::permissions::{
    AuthorizationRequest, Authorizer, Check, Checks, Mode, PermissionInfo, Permissions,
    RequirementInfo, Requirements, ResourceInfo, ResourceRef, Tier, Verdict,
};
use crucible_server_runtime_rust::{
    Crucible, DecodedJob, FrameworkError, Hook, Interceptor, InterceptorExt, InvokeProtocol,
    Operation, Operations, Result,
    operation::{AuthScheme, OperationInfo, Protocol, Segment, ServiceInfo, Unit},
    protocol::Invocation,
};
use serde_json::{Value, json};
use std::sync::Arc;
use weather_service::MemoryCities;

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
    &[],
    false,
    false,
    false,
    None,
    &[],
);
static SERVICE: ServiceInfo = generated::service_info(
    "example.auth#Refusals",
    "1",
    None,
    None,
    &[&INFO],
    &[],
    |_| None,
    None,
    None,
    None,
);

static WORKSPACE: ResourceInfo = ResourceInfo {
    name: "workspace",
    identifiers: &["workspaceId"],
};
static READ: PermissionInfo = PermissionInfo {
    resource: &WORKSPACE,
    symbol: "read",
    value: "workspace:read",
    aliases: &[],
    tier: Tier::Public,
};

/// `requires read on Workspace`, bound to a workspace the input does not
/// carry (it has none), so every protocol's empty request is enough.
struct Probe<const P: u8>;
impl<const P: u8> Operation for Probe<P> {
    const INFO: &'static OperationInfo = &INFO;
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
    const PROTOCOL: Protocol = match P {
        0 => Protocol::CrucibleJson,
        1 => Protocol::AwsRestJson1,
        2 => Protocol::AwsJson10 {
            service: "Refusals",
        },
        3 => Protocol::AwsJson11 {
            service: "Refusals",
        },
        4 => Protocol::SmithyRpcV2Json,
        _ => Protocol::SmithyRpcV2Cbor,
    };
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

async fn check<const P: u8>(invoke: bool) {
    for verdict in [Verdict::Hidden, Verdict::Denied] {
        let operations = Operations::builder(&SERVICE)
            .add::<Probe<P>, _, _>(Arc::new(()), |_, _| async { Ok(Unit {}) })
            .build()
            .unwrap();
        let service = Crucible::builder(operations)
            .defaults()
            .protocol(InvokeProtocol)
            .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
            .interceptor(Permissions::new(Answering(verdict)))
            .build()
            .unwrap();
        let path = if P >= 4 {
            "/service/Refusals/operation/Probe"
        } else if P >= 2 {
            "/"
        } else {
            "/probe"
        };
        let media = match P {
            2 => "application/x-amz-json-1.0",
            3 => "application/x-amz-json-1.1",
            5 => "application/cbor",
            _ => "application/json",
        };
        let mut request = Request::post(path).header("content-type", media);
        if P == 2 || P == 3 {
            request = request.header("x-amz-target", "Refusals.Probe");
        }
        if P >= 4 {
            request = request.header(
                "smithy-protocol",
                if P == 4 { "rpc-v2-json" } else { "rpc-v2-cbor" },
            );
        }
        let body = if P >= 4 && !invoke {
            Body::empty()
        } else {
            Body::from("{}")
        };
        let mut request = request.body(body).unwrap();
        if invoke {
            request.extensions_mut().insert(Invocation {
                operation: "Probe".into(),
            });
        }
        let response = service.handle(request).await;
        let context = format!("protocol {P}, invoke {invoke}, {verdict:?}");
        let headers = response.headers().clone();
        assert!(!headers.contains_key("www-authenticate"), "{context}");
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: Value = if P == 5 && !invoke {
            crucible_server_runtime_rust::__generated::cbor::read(&bytes).unwrap()
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        let value = if invoke { &value["error"] } else { &value };
        let text = value.to_string();
        assert!(!text.contains("w-secret"), "{context}: {text}");
        match verdict {
            Verdict::Hidden => {
                assert_eq!(status, 404, "{context}");
                assert_eq!(headers["cache-control"], "no-store", "{context}");
                let name = if !invoke && matches!(P, 2 | 4 | 5) {
                    "crucible.errors#NotFoundException"
                } else {
                    "NotFoundException"
                };
                assert_eq!(
                    *value,
                    json!({
                        "__type": name,
                        "message": "No such workspace.",
                        "resourceType": "workspace",
                    }),
                    "{context}"
                );
            }
            _ => {
                assert_eq!(status, 403, "{context}");
                let name = if !invoke && P >= 4 {
                    "smithy.framework#ForbiddenException"
                } else {
                    "ForbiddenException"
                };
                assert_eq!(value["__type"], name, "{context}");
                assert_eq!(
                    value["message"],
                    "This principal does not hold workspace:read on that workspace.",
                    "{context}"
                );
            }
        }
    }
}

#[tokio::test]
async fn every_serving_protocol_answers_hidden_as_not_found_and_denied_as_forbidden() {
    check::<0>(false).await;
    check::<1>(false).await;
    check::<2>(false).await;
    check::<3>(false).await;
    check::<4>(false).await;
    check::<5>(false).await;
    check::<0>(true).await;
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
                resource_type: "city",
            },
            false,
        ),
        (
            FrameworkError::PermissionDenied {
                resource_type: "city",
                permission: Some("city:read"),
            },
            true,
        ),
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
