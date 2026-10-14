use std::fmt;

/// One bound identifier of a [`ResourceRef`](super::ResourceRef).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    One(String),
    /// Every instance, including ones created later: `Inbox(*)`. Only a
    /// resource's own (last) identifier is ever `All`.
    All,
}

impl Target {
    /// The identifier, unless it is `All`.
    pub fn as_one(&self) -> Option<&str> {
        match self {
            Self::One(id) => Some(id),
            Self::All => None,
        }
    }
}

impl From<String> for Target {
    fn from(id: String) -> Self {
        Self::One(id)
    }
}

impl From<&String> for Target {
    fn from(id: &String) -> Self {
        Self::One(id.clone())
    }
}

impl From<&str> for Target {
    fn from(id: &str) -> Self {
        Self::One(id.to_owned())
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::One(id) => f.write_str(id),
            Self::All => f.write_str("*"),
        }
    }
}
