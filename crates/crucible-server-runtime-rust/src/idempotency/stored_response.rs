/// What a completed attempt leaves for its retries: the operation's
/// successful output, in its JSON form. A retry gets it encoded afresh, by
/// the protocol and at the API version the retry uses, with the retry's own
/// request id, so the output reads the same way whichever way the retry
/// came. Opaque to a store, which keeps the text as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredResponse {
    output: String,
}

impl StoredResponse {
    /// A response a store kept, as [`StoredResponse::as_str`] gave it.
    pub fn new(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.output
    }
}
