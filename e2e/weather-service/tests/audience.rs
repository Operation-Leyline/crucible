use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use crucible_server_runtime_rust::{
    Crucible, Hook, JsonProtocol, Operation, OperationAwareMiddleware, OperationJob,
};

mod generated {
    include!(concat!(env!("OUT_DIR"), "/audience_server.rs"));
}
use generated::api;

struct Handlers;
impl api::PublicOperation for Handlers {
    async fn public(
        &self,
        input: api::PublicInput,
    ) -> crucible_server_runtime_rust::Result<api::PublicOutput> {
        Ok(api::PublicOutput {
            value: input.value,
            private_value: input.private_value,
            choice: None,
        })
    }
}
impl api::SecretOperation for Handlers {
    async fn secret(&self, _: api::SecretInput) -> crucible_server_runtime_rust::Result<()> {
        Ok(())
    }
}

// An application decides how callers become internal. The framework only
// exposes the lowered annotation; it has no built-in caller authorization.
struct InternalCallers;
impl OperationAwareMiddleware for InternalCallers {
    fn before<'a>(
        &'a self,
        job: &'a mut OperationJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        let internal_caller = job
            .context()
            .headers()
            .get("x-example-caller")
            .is_some_and(|v| v == "internal");
        if job.operation().internal && !internal_caller {
            Hook::Ready(Err(api::PublicFault {
                message: Some("internal caller required".into()),
            }
            .into()))
        } else {
            Hook::Ready(Ok(()))
        }
    }
}

#[tokio::test]
async fn middleware_uses_generated_visibility_and_server_retains_every_operation() {
    assert!(api::Secret::INFO.internal);
    assert!(!api::Public::INFO.internal);
    assert_eq!(api::SERVICE.operations.len(), 2);
    let service = Crucible::builder(api::operations(Arc::new(Handlers)))
        .on_rejection(|rejection| {
            Box::new(api::PublicFault {
                message: Some(rejection.summary()),
            })
        })
        .protocol(JsonProtocol)
        .operation_middleware(InternalCallers)
        .build()
        .unwrap();
    for (path, caller, status) in [
        ("/public", "external", 200),
        ("/secret", "external", 400),
        ("/secret", "internal", 200),
    ] {
        let request = Request::post(path)
            .header("content-type", "application/json")
            .header("x-example-caller", caller)
            .body(Body::from("{}"))
            .unwrap();
        let response = service.handle(request).await;
        assert_eq!(response.status().as_u16(), status, "{path} for {caller}");
    }
}
