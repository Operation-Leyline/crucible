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
    /// (tenant membership, groups). Access tokens are always checked, even if
    /// listed here.
    Exempt(Vec<TokenKind>),
}

impl ScopelessPolicy {
    pub(crate) fn exempts(&self, kind: &TokenKind) -> bool {
        matches!(self, Self::Exempt(kinds) if !matches!(kind, TokenKind::Access) && kinds.contains(kind))
    }
}

#[cfg(test)]
mod tests {
    use super::ScopelessPolicy;
    use crate::TokenKind;

    #[test]
    fn exemptions_preserve_other_token_kinds_but_never_include_access_tokens() {
        let kinds = vec![
            TokenKind::Id,
            TokenKind::ApiKey,
            TokenKind::Other("session".into()),
        ];
        let mut configured = kinds.clone();
        configured.push(TokenKind::Access);
        let policy = ScopelessPolicy::Exempt(configured);
        assert!(!policy.exempts(&TokenKind::Access));
        for kind in kinds {
            assert!(policy.exempts(&kind));
            assert!(!ScopelessPolicy::Refuse.exempts(&kind));
        }
        assert!(!policy.exempts(&TokenKind::Other("unlisted".into())));
    }
}
