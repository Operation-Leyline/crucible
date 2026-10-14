/// How a requirement is enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Mode {
    /// `requires p on R(..)`: checked before the operation runs.
    Check,
    /// `filters p on R`: a listing answered with only the instances the
    /// caller holds `p` on.
    Filter,
    /// `requires p on R by handler`: an obligation the handler discharges,
    /// because only it knows the resource or whether the clause applies.
    Handler,
}
