//! Roles and scopes (docs/roles.md; docs/framework.md, "Roles").
//!
//! A model that declares roles gets a generated `roles` module: closed
//! `Role` and `Scope` enums and `roles::CATALOGUE`, a `'static`
//! [`RoleCatalogue`] holding each role's scopes, holders, assigners and
//! grants (as [`PermissionSet`] bitsets) and each scope's coverage. Those
//! descriptions are always here, so a service may read them from its own
//! `Authorizer`.
//!
//! With the `roles` feature (which enables `permissions`),
//! [`RoleAuthorizer`] is the `Authorizer` that decides from them: the
//! service implements [`BindingSource`] (who the caller is, which bindings
//! it holds, and the `by service` and custom-role hatches) and nothing
//! else. [`ShadowAuthorizer`] runs it beside a service's own Authorizer,
//! and [`conformance`] compares the two over every case a catalogue has.
//!
//! ```ignore
//! let authorizer = RoleAuthorizer::new(&leyline::roles::CATALOGUE, Bindings::new(store));
//! builder.interceptor(Permissions::new(authorizer))
//! ```

mod catalogue;
mod grant;
mod permission_set;
mod role_binding;
mod role_info;
mod scope_info;
mod scope_kind;

#[cfg(feature = "roles")]
mod binding_source;
#[cfg(feature = "roles")]
mod by_service;
#[cfg(feature = "roles")]
mod capabilities;
#[cfg(feature = "roles")]
pub mod conformance;
#[cfg(feature = "roles")]
mod refusal;
#[cfg(feature = "roles")]
mod role_authorizer;
#[cfg(feature = "roles")]
mod root;
#[cfg(feature = "roles")]
mod shadow_authorizer;
#[cfg(feature = "roles")]
mod unavailable;

pub use catalogue::RoleCatalogue;
pub use grant::Grant;
pub use permission_set::PermissionSet;
pub use role_binding::RoleBinding;
pub use role_info::RoleInfo;
pub use scope_info::ScopeInfo;
pub use scope_kind::ScopeKind;

#[cfg(feature = "roles")]
pub use binding_source::BindingSource;
#[cfg(feature = "roles")]
pub use by_service::ByService;
#[cfg(feature = "roles")]
pub use capabilities::Capabilities;
#[cfg(feature = "roles")]
pub use refusal::Refusal;
#[cfg(feature = "roles")]
pub use role_authorizer::RoleAuthorizer;
#[cfg(feature = "roles")]
pub use root::Root;
#[cfg(feature = "roles")]
pub use shadow_authorizer::{ShadowAuthorizer, ShadowCounts};
#[cfg(feature = "roles")]
pub use unavailable::Unavailable;

#[cfg(all(test, feature = "roles"))]
mod tests;
