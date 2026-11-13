use super::{Check, Verdict};
use crate::roles::RoleBinding;

/// One check the Authorizer answered for this request, and its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Decision {
    pub check: Check,
    pub verdict: Verdict,
    /// For an allowed check, the role binding that allowed it, when the
    /// Authorizer names one (`RoleAuthorizer` does): "allowed by
    /// `graphReader` at `allGraphs(o-1)`".
    pub allowed_by: Option<RoleBinding>,
}
