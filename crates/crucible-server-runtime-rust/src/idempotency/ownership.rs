/// Whether a call on a claim found it still held by the attempt that made
/// it. A claim lapses when its request outlives it, and a retry may then
/// take the token, or a store may have dropped the expired record; either
/// way the attempt no longer speaks for the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Ownership {
    /// The attempt still held its in-flight claim, and the call changed it.
    Held,
    /// The attempt no longer held the claim, and the call changed nothing.
    Lost,
}
