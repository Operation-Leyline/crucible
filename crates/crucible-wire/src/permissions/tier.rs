/// Who may see a permission (docs/permissions.md, section 4.1). Every tier
/// reaches the server; only `Internal` stays out of client SDKs, public
/// docs and 403 bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Tier {
    /// Required by operations, granted by callers, documented with its
    /// resource.
    Public,
    /// Public, but required by no operation: a role bundle or a delegation
    /// rule checks it.
    PermissionOnly,
    /// Public, but conferred whole by the service's policy and never
    /// offered for composition (`workspace:admin`).
    NotGrantable,
    /// The server's own vocabulary: absent from every client-facing output,
    /// and never named in a refusal.
    Internal,
}

impl Tier {
    /// Whether a refusal may name a permission of this tier.
    pub const fn is_public(self) -> bool {
        !matches!(self, Self::Internal)
    }
}
