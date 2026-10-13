/// What a request's credential source holds, from this authenticator's view.
pub(crate) enum PresentedCredential<'a> {
    /// Nothing this authenticator would look up; another authenticator may.
    Absent,
    /// A recognized key presented ambiguously (duplicate headers): refused
    /// rather than ignored, so optional auth cannot treat it as anonymous.
    Ambiguous,
    Key(&'a str),
}
