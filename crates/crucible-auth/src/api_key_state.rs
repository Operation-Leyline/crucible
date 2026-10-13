/// Whether a stored key may authenticate. The store maps its own status field
/// onto this; only `Active` admits a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ApiKeyState {
    Active,
    Revoked,
    Disabled,
}
