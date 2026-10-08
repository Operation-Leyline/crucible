use axum::{body::Body, http::Request};
use crucible_server_runtime_rust::{
    Crucible, FrameworkError, InvokeProtocol, Operation, Operations,
    operation::{AuthScheme, OperationInfo, Protocol, Segment, ServiceInfo, Unit},
    protocol::Invocation,
};
use serde_json::Value;
use std::sync::Arc;

static INFO: OperationInfo = OperationInfo {
    id: "example.auth#Probe",
    name: "Probe",
    resource: None,
    internal: false,
    method: "POST",
    path: &[Segment::Literal("refusal")],
    literal_query: &[],
    status: 200,
    input: &[],
    output: &[],
    errors: &[],
    readonly: false,
    idempotent: false,
    idempotency_token: None,
    traits: &[],
};
static SERVICE: ServiceInfo = ServiceInfo {
    mcp_tools: None,
    cors: None,
    id: "example.auth#Refusals",
    version: "1",
    operations: &[&INFO],
    errors: &[],
    classify: |_| None,
    reject: None,
    internal: None,
    unknown: None,
};

struct Probe<const P: u8>;
impl<const P: u8> Operation for Probe<P> {
    const INFO: &'static OperationInfo = &INFO;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[AuthScheme::Bearer]);
    const HAS_INPUT: bool = false;
    const HAS_OUTPUT: bool = false;
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

async fn check<const P: u8>(invoke: bool) {
    for error in [FrameworkError::Unauthenticated, FrameworkError::Forbidden] {
        let operations = Operations::builder(&SERVICE)
            .add::<Probe<P>, _, _>(Arc::new(error.clone()), |error, _| async move {
                Err((*error).clone().into())
            })
            .build()
            .unwrap();
        let service = Crucible::builder(operations)
            .defaults()
            .protocol(InvokeProtocol)
            .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
            .build()
            .unwrap();
        let path = if P >= 4 {
            "/service/Refusals/operation/Probe"
        } else if P >= 2 {
            "/"
        } else {
            "/refusal"
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
        assert_eq!(
            response.status().as_u16(),
            error.status(),
            "protocol {P}, invoke {invoke}"
        );
        assert_eq!(
            response.headers()["www-authenticate"],
            if error == FrameworkError::Forbidden {
                "Bearer error=\"insufficient_scope\""
            } else {
                "Bearer"
            }
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: Value = if P == 5 && !invoke {
            crucible_server_runtime_rust::cbor::read(&bytes).unwrap()
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        let value = if invoke { &value["error"] } else { &value };
        let expected = if P >= 4 && !invoke {
            format!("smithy.framework#{}", error.name())
        } else {
            error.name().to_owned()
        };
        assert_eq!(value["__type"], expected);
    }
}

#[tokio::test]
async fn every_serving_protocol_preserves_typed_authentication_refusals() {
    check::<0>(false).await;
    check::<1>(false).await;
    check::<2>(false).await;
    check::<3>(false).await;
    check::<4>(false).await;
    check::<5>(false).await;
    check::<0>(true).await;
}
