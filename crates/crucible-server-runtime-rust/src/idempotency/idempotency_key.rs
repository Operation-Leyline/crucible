use sha2::{Digest, Sha256};

use super::fingerprint::hex;

/// Whose token a claim is for: the service, the operation, the caller and
/// the token. Two callers that send the same token hold two keys, so one
/// caller's requests never influence another's; nor does a token reused on
/// another operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey {
    service: &'static str,
    operation: &'static str,
    caller: Option<String>,
    token: String,
}

impl IdempotencyKey {
    /// The key of `token`, sent by `caller` to `operation` of `service`
    /// (shape ids). The interceptor builds it from the request; a test or a
    /// store's own tooling may build one to look a record up.
    pub fn new(
        service: &'static str,
        operation: &'static str,
        caller: Option<String>,
        token: impl Into<String>,
    ) -> Self {
        Self {
            service,
            operation,
            caller,
            token: token.into(),
        }
    }

    /// The service's shape id.
    pub fn service(&self) -> &'static str {
        self.service
    }

    /// The operation's shape id.
    pub fn operation(&self) -> &'static str {
        self.operation
    }

    /// Who is asking, as the service's caller binding named them; `None`
    /// for an anonymous caller.
    pub fn caller(&self) -> Option<&str> {
        self.caller.as_deref()
    }

    /// The token the request carried.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// One fixed-length text for the whole key (64 hex digits), for a store
    /// that keeps one partition key. Each part is length-prefixed, so no two
    /// keys digest alike by moving text from one part to the next, and an
    /// anonymous caller never digests like a caller named "".
    pub fn digest(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"crucible idempotency key v1\0");
        for part in [
            self.service.as_bytes(),
            self.operation.as_bytes(),
            self.caller.as_deref().map_or(&b""[..], str::as_bytes),
            self.token.as_bytes(),
        ] {
            digest.update((part.len() as u64).to_be_bytes());
            digest.update(part);
        }
        digest.update([u8::from(self.caller.is_some())]);
        hex(&digest.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::IdempotencyKey;

    fn key(
        service: &'static str,
        operation: &'static str,
        caller: Option<&str>,
        token: &str,
    ) -> IdempotencyKey {
        IdempotencyKey::new(service, operation, caller.map(str::to_owned), token)
    }

    #[test]
    fn every_part_of_the_key_counts() {
        let base = key("s#S", "s#Create", Some("alice"), "t-1");
        assert_eq!(
            base.digest(),
            key("s#S", "s#Create", Some("alice"), "t-1").digest()
        );
        assert_eq!(base.digest().len(), 64);
        for other in [
            key("s#T", "s#Create", Some("alice"), "t-1"),
            key("s#S", "s#Other", Some("alice"), "t-1"),
            key("s#S", "s#Create", Some("bob"), "t-1"),
            key("s#S", "s#Create", None, "t-1"),
            key("s#S", "s#Create", Some(""), "t-1"),
            key("s#S", "s#Create", Some("alice"), "t-2"),
        ] {
            assert_ne!(base.digest(), other.digest(), "{other:?}");
        }
    }

    #[test]
    fn parts_cannot_trade_text() {
        assert_ne!(
            key("s", "o", Some("ab"), "c").digest(),
            key("s", "o", Some("a"), "bc").digest()
        );
    }
}
