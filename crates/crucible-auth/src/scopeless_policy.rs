use crate::TokenKind;

/// What `ScopeEnforcer` does with a principal whose token kind carries no scope
/// claim. Cognito ID tokens (a web client's session) never have one, so
/// enabling enforcement without this choice would refuse every such caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScopelessPolicy {
    /// Every principal must hold the required scopes, whatever its token kind.
    Refuse,
    /// Principals of these token kinds skip scope checks entirely, scopes or no
    /// scopes. The service answers for their authorization some other way
    /// (tenant membership, groups); an access token is never exempted by
    /// default.
    Exempt(Vec<TokenKind>),
}

impl ScopelessPolicy {
    pub(crate) fn exempts(&self, kind: &TokenKind) -> bool {
        matches!(self, Self::Exempt(kinds) if kinds.contains(kind))
    }
}
