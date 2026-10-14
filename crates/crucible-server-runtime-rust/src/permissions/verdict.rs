/// The Authorizer's answer to one [`Check`](super::Check).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Verdict {
    Allowed,
    /// The caller can see the resource and lacks the permission: a 403,
    /// which may name the permission.
    Denied,
    /// The resource is outside the caller's visibility boundary (a
    /// workspace it is not a member of, a key from another tenant): the
    /// framework's 404, identical to a genuinely missing resource. The
    /// container's visibility decides it, never the permission. A Hidden
    /// verdict on a check that binds nothing (a root resource with `*`) has
    /// nothing to hide and is treated as Denied.
    Hidden,
}
