use super::{Check, Verdict};

/// One check the Authorizer answered for this request, and its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Decision {
    pub check: Check,
    pub verdict: Verdict,
}
