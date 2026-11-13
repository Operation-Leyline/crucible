//! The role registry a generated client publishes (`roles::ROLES`,
//! docs/roles.md §7 "Clients"), for a console's role picker. Generated from
//! one rule in every client language: public roles only, in the service's
//! catalogue order, each with its public permissions only.

/// One role in a client's registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RoleDescription {
    /// What bindings store: `graphAdmin`.
    pub value: &'static str,
    /// The model's documentation for it, empty when it has none.
    pub description: &'static str,
    /// The scope values it may be bound at, in catalogue order.
    pub scopes: &'static [&'static str],
    /// The kinds of principal (the service's principals enum values) that
    /// may hold it; empty: any.
    pub holders: &'static [&'static str],
    /// The roles whose holders alone may assign it; empty: whoever may
    /// write bindings.
    pub assigned_by: &'static [&'static str],
    /// What it grants, the model deciding: the permissions a client knows.
    pub permissions: &'static [RolePermission],
}

/// One permission a role grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RolePermission {
    /// The resource type a grant names: `graph`.
    pub resource_type: &'static str,
    /// What grants store: `graph:read`.
    pub permission: &'static str,
}
