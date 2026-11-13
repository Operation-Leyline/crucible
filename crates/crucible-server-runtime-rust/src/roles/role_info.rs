use super::PermissionSet;

/// One role of a [`RoleCatalogue`](super::RoleCatalogue). Generated,
/// `'static`: the IR's `Role` with its grants settled into bitsets.
#[derive(Debug, PartialEq, Eq)]
pub struct RoleInfo {
    /// What the model calls it: `graphAdmin`.
    pub symbol: &'static str,
    /// Its identity, what bindings store: `graphAdmin`.
    pub value: &'static str,
    /// The model's documentation for it, empty when it has none.
    pub description: &'static str,
    /// Server contract only: a client never sees it, so a role picker
    /// leaves it out.
    pub internal: bool,
    /// The scopes it may be bound at (`at [..]`), as indexes into
    /// [`RoleCatalogue::scopes`](super::RoleCatalogue::scopes), in the
    /// order written.
    pub scopes: &'static [u16],
    /// The values of the service's `principals` enum (wire values, `user`)
    /// that may hold it. Empty: every kind of principal.
    pub holders: &'static [&'static str],
    /// The roles whose holders alone may grant or revoke it
    /// (`assignedBy [..]`), as indexes into
    /// [`RoleCatalogue::roles`](super::RoleCatalogue::roles). Empty:
    /// whoever the binding operation admits.
    pub assigned_by: &'static [u16],
    /// What it grants outright, inclusion and `*` expanded.
    pub grants: PermissionSet,
    /// What it grants with the service deciding when (`by service`),
    /// disjoint from `grants`.
    pub by_service: PermissionSet,
}
