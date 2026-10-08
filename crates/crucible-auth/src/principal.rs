use crate::TokenKind;
use serde_json::Value;
use std::collections::BTreeSet;

/// Proof of identity; tenancy, grants and authorization remain application policy.
#[derive(Clone, Debug, PartialEq)]
pub struct Principal {
    pub subject: String,
    pub issuer: String,
    pub client_id: Option<String>,
    pub token_kind: TokenKind,
    pub scopes: BTreeSet<String>,
    pub groups: BTreeSet<String>,
    pub claims: Value,
}
