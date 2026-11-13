//! ScopeBindings: a `BindingSource` over the caller's token.

use std::sync::Arc;

use crucible_auth::{Principal, TokenKind};
use crucible_server_runtime_rust::roles::{BindingSource, RoleBinding, Root, Unavailable};

use crate::leyline::roles::{Role, Scope};

/// The role bindings a caller holds, read from its token's scopes: each
/// `graphs:<level>` scope binds the matching role at every graph. A real
/// service reads a bindings table here; the framework decides from them.
pub struct ScopeBindings;

impl BindingSource for ScopeBindings {
    type Principal = Principal;

    fn kind<'p>(&self, principal: &'p Principal) -> Option<&'p str> {
        Some(match principal.token_kind {
            TokenKind::ApiKey => "apiKey",
            _ => "user",
        })
    }

    async fn bindings(
        &self,
        principal: &Principal,
        _: Root<'_>,
        _: bool,
    ) -> Result<Arc<[RoleBinding]>, Unavailable> {
        let bindings: Vec<RoleBinding> = [
            ("graphs:read", Role::GraphReader),
            ("graphs:write", Role::GraphWriter),
            ("graphs:admin", Role::GraphAdmin),
        ]
        .into_iter()
        .filter(|(scope, _)| principal.scopes.contains(*scope))
        .map(|(_, role)| {
            RoleBinding::new(
                role.as_str(),
                Scope::AllGraphs.as_str(),
                Vec::<String>::new(),
            )
        })
        .collect();
        Ok(bindings.into())
    }
}
