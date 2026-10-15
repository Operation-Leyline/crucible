use crate::TokenKind;
use crucible_server_runtime_rust::Context;
use serde_json::Value;
use std::collections::BTreeSet;

/// Proof of identity; grants and authorization remain application policy.
///
/// Non-exhaustive so identity can grow without breaking authenticators written
/// outside this crate: construct one with [`Principal::new`] and the `with_*`
/// setters.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Principal {
    pub subject: String,
    pub issuer: String,
    pub client_id: Option<String>,
    pub token_kind: TokenKind,
    pub groups: BTreeSet<String>,
    pub claims: Value,
    /// The workspace or organization the credential itself is bound to, set
    /// only from a credential store (an API key's record). A token claim never
    /// sets it: a claim is the identity provider's statement, and tenant
    /// membership for token callers remains the service's check.
    pub tenant: Option<String>,
}

impl Principal {
    /// A principal with no client, groups, tenant or claims (`Value::Null`).
    pub fn new(
        subject: impl Into<String>,
        issuer: impl Into<String>,
        token_kind: TokenKind,
    ) -> Self {
        Self {
            subject: subject.into(),
            issuer: issuer.into(),
            client_id: None,
            token_kind,
            groups: BTreeSet::new(),
            claims: Value::Null,
            tenant: None,
        }
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    pub fn with_groups<S: Into<String>>(mut self, groups: impl IntoIterator<Item = S>) -> Self {
        self.groups = groups.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_claims(mut self, claims: Value) -> Self {
        self.claims = claims;
        self
    }

    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(tenant.into());
        self
    }

    /// Who this principal is, as `Idempotency` and `PageTokens` bind a
    /// token to its caller: the issuer, the subject and the tenant, as a
    /// JSON array, so no two principals encode alike. The tenant is part of
    /// it because one subject may hold credentials bound to two tenants
    /// (two API keys), and a token one of them was issued must not serve
    /// the other.
    ///
    /// A service whose callers are scoped by more than this (an
    /// organization read from a claim, say), where the operation's input
    /// does not carry it, binds its own array that adds it.
    pub fn caller_binding(&self) -> String {
        serde_json::json!([self.issuer, self.subject, self.tenant]).to_string()
    }

    /// The [`caller_binding`](Self::caller_binding) of the request's
    /// authenticated principal, `None` without one: a caller binding to
    /// hand `Idempotency::new` and `PageTokens::new` as it is.
    pub fn caller_binding_of(context: &Context) -> Option<String> {
        context
            .attribute::<Self>()
            .map(|principal| principal.caller_binding())
    }
}

#[cfg(test)]
mod tests {
    use super::Principal;
    use crate::TokenKind;

    #[test]
    fn a_binding_names_the_issuer_subject_and_tenant() {
        let alice = Principal::new("alice", "https://issuer", TokenKind::Access);
        assert_eq!(alice.caller_binding(), r#"["https://issuer","alice",null]"#);
        let bindings = [
            alice.caller_binding(),
            alice.clone().with_tenant("t-1").caller_binding(),
            alice.clone().with_tenant("t-2").caller_binding(),
            alice.clone().with_tenant("").caller_binding(),
            Principal::new("alice", "https://other", TokenKind::Access).caller_binding(),
            Principal::new("bob", "https://issuer", TokenKind::Access).caller_binding(),
            // Moving a separator between the parts is another principal.
            Principal::new("ice", "https://issuer\",\"al", TokenKind::Access).caller_binding(),
        ];
        for (i, one) in bindings.iter().enumerate() {
            for other in &bindings[i + 1..] {
                assert_ne!(one, other);
            }
        }
        // Groups and claims are what the principal may do, not who
        // it is: they change without the caller changing.
        assert_eq!(
            alice.clone().with_groups(["read"]).caller_binding(),
            alice.caller_binding()
        );
    }
}
