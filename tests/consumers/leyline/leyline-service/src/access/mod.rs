//! Authorization: the model's roles, decided by the framework's
//! `RoleAuthorizer` from the bindings `ScopeBindings` reads.

mod scope_bindings;

pub use scope_bindings::ScopeBindings;
