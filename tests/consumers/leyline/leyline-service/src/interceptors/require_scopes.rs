//! RequireScopes: per-operation OAuth scope enforcement.

use std::collections::BTreeMap;

use crucible_auth::Principal;
use crucible_server_runtime_rust::{DecodedJob, FrameworkError, Hook, Interceptor};

/// Requires the verified principal to hold an operation's scope. Operations
/// without a rule need none; public operations never reach a principal.
///
/// This is the local stand-in for `crucible_auth::ScopeEnforcer`, which does
/// this from model-declared scopes; the fixture moves to it once it is on main.
pub struct RequireScopes {
    required: BTreeMap<&'static str, &'static str>,
}

impl RequireScopes {
    pub fn new(rules: impl IntoIterator<Item = (&'static str, &'static str)>) -> Self {
        Self {
            required: rules.into_iter().collect(),
        }
    }
}

impl Interceptor for RequireScopes {
    fn before<'a>(
        &'a self,
        job: &'a mut DecodedJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        let Some(scope) = self.required.get(job.operation().name) else {
            return Hook::Ready(Ok(()));
        };
        let granted = job
            .context()
            .attribute::<Principal>()
            .is_some_and(|principal| principal.scopes.contains(*scope));
        Hook::Ready(if granted {
            Ok(())
        } else {
            Err(FrameworkError::Forbidden.into())
        })
    }
}
