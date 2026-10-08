#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Id,
    Access,
    ApiKey,
    Other(String),
}
