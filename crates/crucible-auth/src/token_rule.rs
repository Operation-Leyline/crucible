use crate::{ClientClaim, TokenKind};

/// A signed token must satisfy one complete rule, including its type discriminator.
#[derive(Clone, Debug)]
pub struct TokenRule {
    pub kind: TokenKind,
    pub client_claim: ClientClaim,
    pub allowed_clients: Vec<String>,
    pub required_claims: Vec<(String, String)>,
    /// Optional JOSE `typ` requirement (for example `at+jwt`).
    pub header_type: Option<String>,
}

impl TokenRule {
    pub fn audience(clients: Vec<String>) -> Self {
        Self {
            kind: TokenKind::Id,
            client_claim: ClientClaim::Audience,
            allowed_clients: clients,
            required_claims: Vec::new(),
            header_type: None,
        }
    }
}
