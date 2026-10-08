/// A nonoptional member was not supplied to a generated model builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct BuildError {
    pub shape: &'static str,
    pub member: &'static str,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: missing required member {}", self.shape, self.member)
    }
}

impl std::error::Error for BuildError {}
