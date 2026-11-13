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
//! service implements [`BindingSource`] (who the caller is and which
//! bindings it holds), plus [`ServiceGrants`] when the model has `by
//! service` grants and [`CustomRoles`] when it has custom roles, and builds
//! it with the generated `roles::authorizer(source)`, which requires
//! exactly those. [`ShadowAuthorizer`] runs it beside a service's own Authorizer,
//! and [`conformance`] compares the two over every case a catalogue has.
//!
//! ```ignore
//! let authorizer = leyline::roles::authorizer(Bindings::new(store));
//! builder.interceptor(Permissions::new(authorizer))
//! ```

#[cfg(feature = "permissions")]
mod assign;
mod catalogue;
mod grant;
mod permission_set;
mod role_binding;
mod role_info;
mod scope_info;
mod scope_kind;
mod scope_ref;

#[cfg(feature = "roles")]
mod binding_source;
#[cfg(feature = "roles")]
mod by_service;
#[cfg(feature = "roles")]
mod capabilities;
#[cfg(feature = "roles")]
pub mod conformance;
#[cfg(feature = "roles")]
mod hatches;
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

#[cfg(feature = "permissions")]
pub use assign::{assign, replaced};
pub use catalogue::RoleCatalogue;
pub use grant::Grant;
pub use permission_set::PermissionSet;
pub use role_binding::RoleBinding;
pub use role_info::RoleInfo;
pub use scope_info::ScopeInfo;
pub use scope_kind::ScopeKind;
pub use scope_ref::ScopeRef;

#[cfg(feature = "roles")]
pub use binding_source::BindingSource;
#[cfg(feature = "roles")]
pub use by_service::ByService;
#[cfg(feature = "roles")]
pub use capabilities::Capabilities;
#[cfg(feature = "roles")]
pub use hatches::{
    BothHatches, CustomHatch, CustomRoles, Hatches, NoHatches, ServiceGrants, ServiceHatch,
};
#[cfg(feature = "roles")]
pub use refusal::Refusal;
#[cfg(feature = "roles")]
pub use role_authorizer::{RoleAuthorizer, role_authorizer};
#[cfg(feature = "roles")]
pub use root::Root;
#[cfg(feature = "roles")]
pub use shadow_authorizer::{ShadowAuthorizer, ShadowCounts};
#[cfg(feature = "roles")]
pub use unavailable::Unavailable;

#[cfg(all(test, feature = "roles"))]
mod tests;
