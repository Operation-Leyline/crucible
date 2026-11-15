use crucible_server_runtime_rust::Result;
use crucible_server_runtime_rust::permissions::{AuthorizationRequest, Authorizer, Check, Verdict};

pub struct Grants(pub bool);

#[crucible_server_runtime_rust::async_trait]
impl Authorizer for Grants {
    async fn decide(&self, _: &AuthorizationRequest<'_>, checks: &[Check]) -> Result<Vec<Verdict>> {
        Ok(checks
            .iter()
            .map(|_| {
                if self.0 {
                    Verdict::Allowed
                } else {
                    Verdict::Denied
                }
            })
            .collect())
    }
}
