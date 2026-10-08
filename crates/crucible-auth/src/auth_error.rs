#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid authentication configuration: {0}")]
    Configuration(String),
    #[error("identity provider unavailable: {0}")]
    Unavailable(String),
    #[error("credential refused")]
    Refused,
}
