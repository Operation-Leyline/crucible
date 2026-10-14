use super::{Decision, Listing, ObligationRecord, Requirements, Visibility};

/// Everything authorization decided for one request, left on the request
/// context (`context.attribute::<AuthorizationRecord>()`) for audit
/// middleware that runs later. The audited action is each decision's check:
/// the pair (resource type, permission value). Handler checks are added as
/// they are made.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AuthorizationRecord {
    /// The operation's shape id.
    pub operation: &'static str,
    pub requirements: &'static Requirements,
    /// Every check the Authorizer answered, declared ones first, in the
    /// order they were made.
    pub decisions: Vec<Decision>,
    /// A `filters` listing, and what the Authorizer said it may show.
    pub listing: Option<(Listing, Visibility)>,
    /// The `requires pending` reason the operation passed through with.
    pub pending: Option<&'static str>,
    /// Whether the operation declared nothing and passed anyway: it is
    /// public, `allow_undeclared` let it through, or the interceptor only
    /// shadows.
    pub undeclared: bool,
    pub obligations: Vec<ObligationRecord>,
    /// Whether the interceptor only shadowed: verdicts were recorded and
    /// logged, never enforced.
    pub shadow: bool,
}
