use std::fmt;

use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use hkdf::Hkdf;
use sha2::Sha256;

use super::PageKeyError;

/// The shortest secret a key is derived from.
const MIN_SECRET: usize = 32;

/// Separates page token keys from anything else derived from the same
/// secret.
const DERIVATION: &[u8] = b"crucible page tokens v1";

/// The keys page tokens are sealed and opened with: one current key, which
/// seals, and any number of retired ones, which still open the tokens they
/// sealed until those expire.
///
/// Each key is derived from a secret of at least 32 bytes (HKDF-SHA256), so
/// any random string from a secret store will do. Rotating is: seal with a
/// new key and accept the old one for one token lifetime, then drop it.
#[derive(Clone)]
pub struct PageKeys {
    current: PageKey,
    accepted: Vec<PageKey>,
}

#[derive(Clone)]
pub(crate) struct PageKey {
    pub(crate) id: String,
    pub(crate) cipher: XChaCha20Poly1305,
}

impl PageKeys {
    /// Seals with this key, and opens what it sealed.
    pub fn new(id: impl Into<String>, secret: &[u8]) -> Result<Self, PageKeyError> {
        Ok(Self {
            current: PageKey::derive(id.into(), secret)?,
            accepted: Vec::new(),
        })
    }

    /// Also opens tokens sealed with this (retired) key.
    pub fn accept(mut self, id: impl Into<String>, secret: &[u8]) -> Result<Self, PageKeyError> {
        let key = PageKey::derive(id.into(), secret)?;
        if self.find(&key.id).is_some() {
            return Err(PageKeyError::DuplicateId);
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
        let valid_id = (1..=16).contains(&id.len())
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
        if !valid_id {
            return Err(PageKeyError::InvalidId);
        }
        if secret.len() < MIN_SECRET {
            return Err(PageKeyError::SecretTooShort);
        }
        let mut key = [0u8; 32];
        Hkdf::<Sha256>::new(None, secret)
            .expand(DERIVATION, &mut key)
            .expect("32 bytes is a valid HKDF-SHA256 output length");
        Ok(Self {
            id,
            cipher: XChaCha20Poly1305::new(&key.into()),
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
    fn debug_shows_ids_only() {
        let keys = PageKeys::new("k2", SECRET)
            .unwrap()
            .accept("k1", SECRET)
            .unwrap();
        assert_eq!(
            format!("{keys:?}"),
            r#"PageKeys { current: "k2", accepted: ["k1"] }"#
        );
    }
}
