use std::fmt;

/// Why a page token key was refused at construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageKeyError {
    /// The secret is shorter than 32 bytes.
    SecretTooShort,
    /// A key id is 1 to 16 characters of `A-Z`, `a-z`, `0-9`, `_` and `-`:
    /// it travels in every token.
    InvalidId,
    /// Two keys share an id, so a token could not say which one sealed it.
    DuplicateId,
}

impl fmt::Display for PageKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::SecretTooShort => "a page token secret is at least 32 bytes",
            Self::InvalidId => {
                "a page token key id is 1 to 16 characters of A-Z, a-z, 0-9, '_' and '-'"
            }
            Self::DuplicateId => "two page token keys share an id",
        })
    }
}

impl std::error::Error for PageKeyError {}
