use super::Tier;

/// One resource's permissions as a generated client publishes them
/// (`permissions::PERMISSIONS`), for a console's grant picker. Clients
/// never carry an internal permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourcePermissions {
    /// The authorization name grants store with each value: `inbox`.
    pub resource_type: &'static str,
    /// Sorted by value, deprecated aliases included.
    pub permissions: &'static [PermissionDescription],
}

/// One permission in a client's registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PermissionDescription {
    /// What grants store: `mail:send`.
    pub value: &'static str,
    /// The model's documentation for it, empty when it has none.
    pub description: &'static str,
    pub tier: Tier,
    /// Whether a console may offer it for a grant: false for a bundle the
    /// service's policy confers whole (`workspace:admin`).
    pub grantable: bool,
    pub deprecated: bool,
    /// For a deprecated alias, the value it stands for.
    pub alias_of: Option<&'static str>,
}
