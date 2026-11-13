//! Roles and scopes (docs/roles.md §4): the role catalogue a service
//! declares over its typed permissions. The `.cru` lowering declares them;
//! [`crate::roles`] settles what they mean (coverage, effective grants) once,
//! and `crate::validate::roles` checks them.
//!
//! The serde names are part of the release snapshot format: every field is
//! `#[serde(default)]`, and the service's fields are not written when empty,
//! so a release frozen before roles existed still reads and a model without
//! roles writes the snapshot it wrote before.

use crate::doc_notes::DocNotes;
use crate::shape_id::ShapeId;

/// `scope name = "value" on ..`: a place a role binding is made at.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Scope {
    /// What the model calls it: `allGraphs`.
    #[serde(default)]
    pub symbol: String,
    /// Its identity, stored in bindings: `"allGraphs"`.
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub on: ScopeTarget,
    /// Server contract only: absent from every client-facing output.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub internal: bool,
    #[serde(default)]
    pub docs: Option<String>,
    /// `@deprecated`, `@since`.
    #[serde(default)]
    pub notes: DocNotes,
}

/// What a scope's instance names.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ScopeTarget {
    /// `on service`: the whole service; an instance carries no identifier.
    #[default]
    Service,
    /// `on R`: one instance of `R`, named by all its identifiers.
    Instance(ShapeId),
    /// `on R(*)`: every instance of `R` under one parent instance, those
    /// created later included; named by the parent's identifiers.
    Every(ShapeId),
}

impl ScopeTarget {
    /// The resource the scope is on, if any.
    pub fn resource(&self) -> Option<&ShapeId> {
        match self {
            Self::Service => None,
            Self::Instance(id) | Self::Every(id) => Some(id),
        }
    }
}

/// `role name = "value" { .. }`: a named set of permissions, bound at scopes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Role {
    /// What the model calls it: `graphAdmin`.
    #[serde(default)]
    pub symbol: String,
    /// Its identity, stored in bindings: `"graphAdmin"`.
    #[serde(default)]
    pub value: String,
    /// Server contract only: absent from every client-facing output.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub internal: bool,
    #[serde(default)]
    pub docs: Option<String>,
    /// `@deprecated`, `@since`.
    #[serde(default)]
    pub notes: DocNotes,
    /// `at [..]`: the scopes it may be bound at, by symbol, as written.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// `includes [..]`: the roles whose grants it adds, by symbol, as
    /// written.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<String>,
    /// `grants ..` as written, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<Grant>,
    /// `holders [..]`: the values of the service's `principals` enum, by
    /// name, that may hold it. Empty: every kind of principal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holders: Vec<String>,
    /// `assignedBy [..]`: the roles whose holders alone may grant or revoke
    /// it, by symbol. Empty: whoever the binding operation admits.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assigned_by: Vec<String>,
    /// What it grants, the model deciding: its own grants and its included
    /// roles', `*` expanded, `by service` grants left out. Each permission
    /// once, by primary symbol, sorted by resource id and then in the
    /// resource's declaration order. Settled by [`crate::roles::settle`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effective: Vec<GrantedPermission>,
    /// What it grants with the service deciding (`by service`), its own and
    /// its included roles', less what `effective` already grants; sorted as
    /// `effective` is. Settled by [`crate::roles::settle`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_service: Vec<ServiceGrant>,
}

/// One `grants R [..]` entry, as written.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Grant {
    pub resource: ShapeId,
    #[serde(default)]
    pub permissions: GrantedSet,
    /// `by service`: the service decides when it applies (§4.6).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub by_service: bool,
    /// The entry's `///`: for a `by service` grant, when it applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
}

/// Which of a resource's permissions a grant names.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GrantedSet {
    /// `[p, q]`: these, by symbol, as written.
    Named(Vec<String>),
    /// `[*]` or `[* except [p, q]]`: every permission of the resource that
    /// is not `@permissionOnly` (nor an alias), less these.
    All { except: Vec<String> },
}

impl Default for GrantedSet {
    fn default() -> Self {
        Self::Named(Vec::new())
    }
}

/// One permission a role grants.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct GrantedPermission {
    pub resource: ShapeId,
    /// The primary's symbol.
    #[serde(default)]
    pub permission: String,
}

/// One permission a role grants with the service deciding when.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ServiceGrant {
    pub resource: ShapeId,
    /// The primary's symbol.
    #[serde(default)]
    pub permission: String,
    /// The `///` of the grant that gives it, when it applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
}

/// The service's role settings beside its catalogue (`Service::role_settings`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RoleSettings {
    /// `roles [..] custom`: customers may define further roles at runtime
    /// (§4.7). `roles.ungranted` is off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub custom: bool,
    /// `roles [..] direct`: the service also grants permissions directly,
    /// outside any role, so `roles.ungranted` is off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub direct: bool,
    /// `principals E`: the enum whose values `holders` names (§4.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principals: Option<ShapeId>,
    /// The `@roleTypes` enum: a hand-written wire enum of role values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_types: Option<ShapeId>,
    /// The `@scopeTypes` enum: a hand-written wire enum of scope values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_types: Option<ShapeId>,
}

impl RoleSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}
