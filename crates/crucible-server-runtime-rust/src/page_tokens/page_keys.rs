use std::fmt;

use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use hkdf::Hkdf;
use sha2::Sha256;

use super::PageKeyError;

/// The shortest secret a key is derived from.
const MIN_SECRET: usize = 32;

/// The longest key id: it travels in every token.
pub(crate) const MAX_ID: usize = 16;

/// Separates page token keys from anything else derived from the same
/// secret.
const DERIVATION: &[u8] = b"crucible page tokens v1";

/// Derives what tells two secrets apart without keeping either: a separate
/// HKDF output, so it reveals nothing about the key.
const FINGERPRINT: &[u8] = b"crucible page tokens v1 fingerprint";

/// The keys page tokens are sealed and opened with: one current key, which
/// seals, and any number of retired ones, which still open the tokens they
/// sealed until those expire.
///
/// Each key is derived from a secret of at least 32 bytes (HKDF-SHA256). The
/// secret must be random (32 bytes from a CSPRNG, or a random string from a
/// secret store), never a password: its length is checked, its randomness
/// cannot be.
///
/// Rotating is: seal with a new key, from a new secret, and accept the old
/// one for one token lifetime, then drop it. A new id over the same secret
/// is the same key under another name, so it is refused. Dropping a key is
/// the only way to revoke the tokens it sealed: their expiry is sealed
/// inside, so a shorter [`ttl`](super::PageTokens::ttl) does not reach them.
#[derive(Clone)]
pub struct PageKeys {
    current: PageKey,
    accepted: Vec<PageKey>,
}

#[derive(Clone)]
pub(crate) struct PageKey {
    pub(crate) id: String,
    pub(crate) cipher: XChaCha20Poly1305,
    fingerprint: [u8; 32],
}

impl PageKeys {
    /// Seals with this key, and opens what it sealed.
    pub fn new(id: impl Into<String>, secret: &[u8]) -> Result<Self, PageKeyError> {
        Ok(Self {
            current: PageKey::derive(id.into(), secret)?,
            accepted: Vec::new(),
        })
    }

    /// Also opens tokens sealed with this (retired) key. Its id and its
    /// secret must both differ from every other key's.
    pub fn accept(mut self, id: impl Into<String>, secret: &[u8]) -> Result<Self, PageKeyError> {
        let key = PageKey::derive(id.into(), secret)?;
        if self.find(&key.id).is_some() {
            return Err(PageKeyError::DuplicateId);
        }
        let reused = std::iter::once(&self.current)
            .chain(&self.accepted)
            .any(|other| other.fingerprint == key.fingerprint);
        if reused {
            return Err(PageKeyError::ReusedSecret);
        }
        self.accepted.push(key);
        Ok(self)
    }

    pub(crate) fn current(&self) -> &PageKey {
        &self.current
    }

    pub(crate) fn find(&self, id: &str) -> Option<&PageKey> {
        std::iter::once(&self.current)
            .chain(&self.accepted)
            .find(|key| key.id == id)
    }
}

impl PageKey {
    fn derive(id: String, secret: &[u8]) -> Result<Self, PageKeyError> {
        let valid_id = (1..=MAX_ID).contains(&id.len())
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
        if !valid_id {
            return Err(PageKeyError::InvalidId);
        }
        if secret.len() < MIN_SECRET {
            return Err(PageKeyError::SecretTooShort);
        }
        let hkdf = Hkdf::<Sha256>::new(None, secret);
        let mut key = [0u8; 32];
        let mut fingerprint = [0u8; 32];
        for (info, out) in [(DERIVATION, &mut key), (FINGERPRINT, &mut fingerprint)] {
            hkdf.expand(info, out)
                .expect("32 bytes is a valid HKDF-SHA256 output length");
        }
        Ok(Self {
            id,
            cipher: XChaCha20Poly1305::new(&key.into()),
            fingerprint,
        })
    }
}

/// Names the keys, never their material.
impl fmt::Debug for PageKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageKeys")
            .field("current", &self.current.id)
            .field(
                "accepted",
                &self.accepted.iter().map(|key| &key.id).collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";
    const OTHER: &[u8] = b"fedcba9876543210fedcba9876543210";

    #[test]
    fn a_short_secret_is_refused() {
        assert_eq!(
            PageKeys::new("k1", &SECRET[..31]).unwrap_err(),
            PageKeyError::SecretTooShort
        );
    }

    #[test]
    fn an_id_must_travel_safely_in_a_token() {
        for id in ["", "has.dot", "has space", "seventeen-chars-x", "é"] {
            assert_eq!(
                PageKeys::new(id, SECRET).unwrap_err(),
                PageKeyError::InvalidId,
                "{id:?}"
            );
        }
        PageKeys::new("Key_2026-10", SECRET).unwrap();
    }

    #[test]
    fn ids_are_unique() {
        let keys = PageKeys::new("k1", SECRET).unwrap();
        assert_eq!(
            keys.accept("k1", SECRET).unwrap_err(),
            PageKeyError::DuplicateId
        );
    }

    #[test]
    fn rotating_needs_a_new_secret() {
        let keys = PageKeys::new("k2", SECRET).unwrap();
        assert_eq!(
            keys.clone().accept("k1", SECRET).unwrap_err(),
            PageKeyError::ReusedSecret
        );
        let keys = keys.accept("k1", OTHER).unwrap();
        assert_eq!(
            keys.accept("k0", OTHER).unwrap_err(),
            PageKeyError::ReusedSecret
        );
    }

    #[test]
    fn debug_shows_ids_only() {
        let keys = PageKeys::new("k2", SECRET)
            .unwrap()
            .accept("k1", OTHER)
            .unwrap();
        assert_eq!(
            format!("{keys:?}"),
            r#"PageKeys { current: "k2", accepted: ["k1"] }"#
        );
    }
}
