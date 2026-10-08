use std::sync::Arc;

use crucible_auth::{Authenticator, AuthenticatorChain, ScopeEnforcer};
use crucible_server_runtime_rust::{Crucible, InvokeProtocol, JsonProtocol};

use crate::leyline::{self, AccessDeniedException};
use crate::{Audit, Leyline, RequestTag, TenantGate};

/// The service over a configured set of authenticators (JWT verification in
/// production, a test issuer in tests) and an audit trail.
pub fn service(authenticators: Vec<Arc<dyn Authenticator>>, audit: Audit) -> Crucible {
    // The chain keeps Leyline's modelled refusal body, so a missing and an
    // incorrect credential look alike while the bearer challenge is retained.
    let chain = AuthenticatorChain::new(authenticators).refusal_factory(|_| {
        AccessDeniedException {
            message: "access denied".into(),
        }
        .into()
    });
    Crucible::builder(leyline::operations(Arc::new(Leyline::default())))
        .middleware(RequestTag)
        .operation_middleware(TenantGate)
        .operation_middleware(chain)
        .interceptor(ScopeEnforcer::strict())
        .interceptor(audit)
        .protocol(JsonProtocol)
        // Direct Lambda invokes: `{"operation": .., "input": ..}`.
        .protocol(InvokeProtocol)
        .on_rejection(
            leyline::SERVICE
                .reject
                .expect("the model declares a validation error"),
        )
        .build()
        .expect("the Leyline service builds")
}
