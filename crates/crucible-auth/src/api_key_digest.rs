use sha2::{Digest, Sha256};
use std::fmt::Write;

/// The lookup key for a credential: lowercase hex SHA-256 of the whole
/// credential, prefix included. Services store this when minting a key so
/// minting and checking cannot disagree; the credential itself is never stored.
///
/// A fast unsalted hash is enough here because a key is high-entropy random
/// data, not a password: there is nothing to guess from the digest.
pub fn api_key_digest(credential: &str) -> String {
    let hash = Sha256::digest(credential.as_bytes());
    let mut hex = String::with_capacity(hash.len() * 2);
    for byte in hash {
        // Writing to a String cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}
