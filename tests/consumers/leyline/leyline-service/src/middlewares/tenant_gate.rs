//! TenantGate: operation-aware admission.

use crucible_server_runtime_rust::{FrameworkError, Hook, OperationAwareMiddleware, OperationJob};

/// Refuses suspended tenants once the operation is known and before the body
/// is read, so invalid input from a suspended tenant is still refused.
pub struct TenantGate;

impl OperationAwareMiddleware for TenantGate {
    fn before<'a>(
        &'a self,
        job: &'a mut OperationJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        let suspended = job
            .request()
            .headers
            .get("x-leyline-tenant")
            .is_some_and(|tenant| tenant == "suspended");
        Hook::Ready(if suspended {
            Err(FrameworkError::Forbidden.into())
        } else {
            Ok(())
        })
    }
}
