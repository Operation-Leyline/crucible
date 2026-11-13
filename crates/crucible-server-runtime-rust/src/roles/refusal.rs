use crate::permissions::Verdict;

/// How `RoleAuthorizer` refuses a check no binding allows:
/// [`BindingSource::refusal`](super::BindingSource::refusal) chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// A 404: the target is outside what the caller may know exists.
    Hidden,
    /// A 403: the caller may see the target and lacks the permission.
    Denied,
}

impl From<Refusal> for Verdict {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::Hidden => Verdict::Hidden,
            Refusal::Denied => Verdict::Denied,
        }
    }
}
