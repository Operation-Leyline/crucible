use axum::{body::Body, http::Request};
use crucible_server_runtime_rust::__generated as generated;
use crucible_server_runtime_rust::{
    Crucible, FrameworkError, InvokeProtocol, Operation, Operations,
    operation::{AuthScheme, OperationInfo, Segment, ServiceInfo, Unit},
    protocol::Invocation,
};
use serde_json::Value;
use std::sync::Arc;

static INFO: OperationInfo = generated::operation_info(
    "example.auth#Probe",
    "Probe",
    None,
    "POST",
    &[Segment::Literal("refusal")],
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
    &[&INFO],
    &[],
    |_| None,
    None,
    None,
    None,
);

struct Probe;
impl Operation for Probe {
    const INFO: &'static OperationInfo = &INFO;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[AuthScheme::Bearer]);
    const HAS_INPUT: bool = false;
    const HAS_OUTPUT: bool = false;
    type Input = Unit;
    type Output = Unit;
}

async fn check(invoke: bool) {
    for error in [FrameworkError::Unauthenticated, FrameworkError::Forbidden] {
        let operations = Operations::builder(&SERVICE)
            .add::<Probe, _, _>(Arc::new(error.clone()), |error, _| async move {
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
        let mut request = Request::post("/refusal")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        if invoke {
            request.extensions_mut().insert(Invocation {
                operation: "Probe".into(),
            });
        }
        let response = service.handle(request).await;
        assert_eq!(
            response.status().as_u16(),
            error.status(),
            "invoke {invoke}"
        );
        // Only a missing or bad credential is challenged: a 403 names no
        // credential to go and get.
        assert_eq!(
            response
                .headers()
                .get("www-authenticate")
                .map(|v| v.to_str().unwrap()),
            (error == FrameworkError::Unauthenticated).then_some("Bearer")
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let value = if invoke { &value["error"] } else { &value };
        assert_eq!(value["__type"], error.name());
    }
}

#[tokio::test]
async fn typed_authentication_refusals_are_preserved() {
    check(false).await;
    check(true).await;
}
