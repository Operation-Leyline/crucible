use std::fmt;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::canonical_json::write_canonical;

/// What a request asked for, as a digest: the decoded input without its
/// idempotency token, in canonical JSON, so the same input fingerprints
/// alike whichever protocol, binding or member order carried it. A token
/// reused with another fingerprint is another request.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// The fingerprint of `input`, the decoded input's JSON form without
    /// its token.
    pub(crate) fn of(input: &Value) -> Self {
        let mut canonical = Vec::new();
        write_canonical(input, &mut canonical);
        let mut digest = Sha256::new();
        digest.update(b"crucible idempotency fingerprint v1\0");
        digest.update(&canonical);
        Self(digest.finalize().into())
    }

    /// A fingerprint a store kept, as [`Fingerprint::as_bytes`] gave it.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// A fingerprint a store kept as [`Fingerprint::to_hex`] gave it, or
    /// `None` when `text` is not 64 hex digits.
    pub fn from_hex(text: &str) -> Option<Self> {
        let text = text.as_bytes();
        if text.len() != 64 {
            return None;
        }
        let digit = |byte: u8| (byte as char).to_digit(16);
        let mut bytes = [0u8; 32];
        for (index, &[high, low]) in text.as_chunks::<2>().0.iter().enumerate() {
            bytes[index] = u8::try_from(digit(high)? * 16 + digit(low)?).ok()?;
        }
        Some(Self(bytes))
    }

    /// Lowercase hex, for a store that keeps text.
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[usize::from(byte >> 4)] as char);
        text.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    text
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({})", self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Fingerprint;

    #[test]
    fn the_same_input_in_another_order_fingerprints_alike() {
        let one: serde_json::Value =
            serde_json::from_str(r#"{"name":"Oslo","location":{"lat":1,"lon":2}}"#).unwrap();
        let two: serde_json::Value =
            serde_json::from_str(r#"{"location":{"lon":2,"lat":1},"name":"Oslo"}"#).unwrap();
        assert_eq!(Fingerprint::of(&one), Fingerprint::of(&two));
    }

    #[test]
    fn any_difference_in_the_input_fingerprints_apart() {
        let base = Fingerprint::of(&json!({"name": "Oslo", "tags": ["a", "b"]}));
        for other in [
            json!({"name": "Bergen", "tags": ["a", "b"]}),
            json!({"name": "Oslo", "tags": ["b", "a"]}),
            json!({"name": "Oslo"}),
            json!({"name": "Oslo", "tags": ["a", "b"], "extra": null}),
        ] {
            assert_ne!(base, Fingerprint::of(&other), "{other}");
        }
    }

    #[test]
    fn hex_round_trips() {
        let fingerprint = Fingerprint::of(&json!({"a": 1}));
        let text = fingerprint.to_hex();
        assert_eq!(text.len(), 64);
        assert_eq!(Fingerprint::from_hex(&text), Some(fingerprint));
        assert_eq!(
            Fingerprint::from_hex(&text.to_uppercase()),
            Some(fingerprint)
        );
        assert_eq!(Fingerprint::from_hex("00"), None);
        assert_eq!(Fingerprint::from_hex(&"zz".repeat(32)), None);
        assert_eq!(
            Fingerprint::from_bytes(*fingerprint.as_bytes()),
            fingerprint
        );
    }
}
