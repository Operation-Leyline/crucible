use crate::TokenKind;
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
    pub scopes: BTreeSet<String>,
    pub groups: BTreeSet<String>,
    pub claims: Value,
    /// The workspace or organization the credential itself is bound to, set
    /// only from a credential store (an API key's record). A token claim never
    /// sets it: a claim is the identity provider's statement, and tenant
    /// membership for token callers remains the service's check.
    pub tenant: Option<String>,
}

impl Principal {
    /// A principal with no client, scopes, groups, tenant or claims (`Value::Null`).
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
            scopes: BTreeSet::new(),
            groups: BTreeSet::new(),
            claims: Value::Null,
            tenant: None,
        }
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    pub fn with_scopes<S: Into<String>>(mut self, scopes: impl IntoIterator<Item = S>) -> Self {
        self.scopes = scopes.into_iter().map(Into::into).collect();
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
}
