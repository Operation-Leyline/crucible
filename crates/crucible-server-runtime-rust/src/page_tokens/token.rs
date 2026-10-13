//! The token format: `p1.<key id>.<base64url(nonce ‖ ciphertext)>`.
//!
//! The plaintext is the expiry (seconds since the epoch, big-endian) and the
//! service's cursor. The associated data is the format, the key id and the
//! request binding, so a token moved to another listing, caller or key, or
//! relabelled with another format, does not open.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, AeadCore, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use super::PageKeys;

/// The format. Anything else is refused rather than read as this one.
const VERSION: &str = "p1";

const NONCE: usize = 24;
const TAG: usize = 16;
const EXPIRY: usize = 8;

pub(crate) type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// What every token a service issues is sealed with.
#[derive(Clone)]
pub(crate) struct Sealer {
    pub(crate) keys: PageKeys,
    pub(crate) ttl: Duration,
    pub(crate) clock: Clock,
}

/// A token that did not open. Deliberately without a reason: every case has
/// the same remedy, and the reason would tell a forger which check failed.
#[derive(Debug)]
pub(crate) struct Invalid;

impl Sealer {
    pub(crate) fn seal(&self, binding: &[u8; 32], cursor: &[u8]) -> Option<String> {
        let key = self.keys.current();
        let expires = self.now().saturating_add(self.ttl.as_secs());
        let mut plain = Vec::with_capacity(EXPIRY + cursor.len());
        plain.extend_from_slice(&expires.to_be_bytes());
        plain.extend_from_slice(cursor);
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let sealed = key
            .cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: &plain,
                    aad: &associated(&key.id, binding),
                },
            )
            .ok()?;
        let mut bytes = nonce.to_vec();
        bytes.extend_from_slice(&sealed);
        Some(format!(
            "{VERSION}.{}.{}",
            key.id,
            URL_SAFE_NO_PAD.encode(bytes)
        ))
    }

    pub(crate) fn open(&self, binding: &[u8; 32], token: &str) -> Result<Vec<u8>, Invalid> {
        let mut parts = token.splitn(3, '.');
        let (Some(VERSION), Some(id), Some(payload)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(Invalid);
        };
        let key = self.keys.find(id).ok_or(Invalid)?;
        let bytes = URL_SAFE_NO_PAD.decode(payload).map_err(|_| Invalid)?;
        if bytes.len() < NONCE + TAG + EXPIRY {
            return Err(Invalid);
        }
        let (nonce, sealed) = bytes.split_at(NONCE);
        let plain = key
            .cipher
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: sealed,
                    aad: &associated(id, binding),
                },
            )
            .map_err(|_| Invalid)?;
        let (expires, cursor) = plain.split_at(EXPIRY);
        let expires = u64::from_be_bytes(expires.try_into().map_err(|_| Invalid)?);
        if self.now() > expires {
            return Err(Invalid);
        }
        Ok(cursor.to_vec())
    }

    fn now(&self) -> u64 {
        (self.clock)()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs())
    }
}

fn associated(id: &str, binding: &[u8; 32]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(VERSION.len() + id.len() + 2 + binding.len());
    aad.extend_from_slice(VERSION.as_bytes());
    aad.push(0);
    aad.extend_from_slice(id.as_bytes());
    aad.push(0);
    aad.extend_from_slice(binding);
    aad
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";
    const OTHER: &[u8] = b"fedcba9876543210fedcba9876543210";
    const BINDING: [u8; 32] = [7; 32];

    fn sealer(keys: PageKeys) -> Sealer {
        Sealer {
            keys,
            ttl: Duration::from_secs(60),
            clock: Arc::new(SystemTime::now),
        }
    }

    fn keys(id: &str, secret: &[u8]) -> PageKeys {
        PageKeys::new(id, secret).unwrap()
    }

    #[test]
    fn a_cursor_survives_a_round_trip() {
        let sealer = sealer(keys("k1", SECRET));
        let token = sealer.seal(&BINDING, b"city-42").unwrap();
        assert!(token.starts_with("p1.k1."));
        assert!(
            token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        );
        assert_eq!(sealer.open(&BINDING, &token).unwrap(), b"city-42");
    }

    #[test]
    fn the_cursor_is_not_readable() {
        let token = sealer(keys("k1", SECRET))
            .seal(&BINDING, b"plain-cursor")
            .unwrap();
        let payload = URL_SAFE_NO_PAD
            .decode(token.rsplit('.').next().unwrap())
            .unwrap();
        assert!(!payload.windows(12).any(|window| window == b"plain-cursor"));
    }

    #[test]
    fn two_tokens_for_one_cursor_differ() {
        let sealer = sealer(keys("k1", SECRET));
        assert_ne!(
            sealer.seal(&BINDING, b"c").unwrap(),
            sealer.seal(&BINDING, b"c").unwrap()
        );
    }

    #[test]
    fn a_token_opens_only_for_its_binding() {
        let sealer = sealer(keys("k1", SECRET));
        let token = sealer.seal(&BINDING, b"c").unwrap();
        assert!(sealer.open(&[8; 32], &token).is_err());
    }

    #[test]
    fn any_edit_is_refused() {
        let sealer = sealer(keys("k1", SECRET));
        let token = sealer.seal(&BINDING, b"cursor").unwrap();
        let (head, payload) = token.rsplit_once('.').unwrap();
        let mut bytes = URL_SAFE_NO_PAD.decode(payload).unwrap();
        for index in 0..bytes.len() {
            bytes[index] ^= 1;
            let edited = format!("{head}.{}", URL_SAFE_NO_PAD.encode(&bytes));
            assert!(sealer.open(&BINDING, &edited).is_err(), "byte {index}");
            bytes[index] ^= 1;
        }
        for edited in [
            token.replacen("p1.", "p2.", 1),
            token.replacen(".k1.", ".k2.", 1),
            format!("{token}x"),
            "p1.k1.".to_owned(),
            "p1".to_owned(),
            String::new(),
        ] {
            assert!(sealer.open(&BINDING, &edited).is_err(), "{edited}");
        }
    }

    #[test]
    fn a_token_expires() {
        let now = Arc::new(AtomicU64::new(1_000_000));
        let clock = {
            let now = now.clone();
            Arc::new(move || UNIX_EPOCH + Duration::from_secs(now.load(Ordering::Relaxed)))
        };
        let sealer = Sealer {
            keys: keys("k1", SECRET),
            ttl: Duration::from_secs(60),
            clock,
        };
        let token = sealer.seal(&BINDING, b"c").unwrap();
        now.store(1_000_060, Ordering::Relaxed);
        assert!(sealer.open(&BINDING, &token).is_ok());
        now.store(1_000_061, Ordering::Relaxed);
        assert!(sealer.open(&BINDING, &token).is_err());
    }

    #[test]
    fn a_retired_key_still_opens_what_it_sealed() {
        let old = sealer(keys("k1", SECRET));
        let token = old.seal(&BINDING, b"c").unwrap();

        let rotated = sealer(keys("k2", OTHER).accept("k1", SECRET).unwrap());
        assert_eq!(rotated.open(&BINDING, &token).unwrap(), b"c");
        assert!(rotated.seal(&BINDING, b"c").unwrap().starts_with("p1.k2."));

        let dropped = sealer(keys("k2", OTHER));
        assert!(dropped.open(&BINDING, &token).is_err());
    }

    #[test]
    fn a_key_with_the_same_id_and_another_secret_does_not_open() {
        let token = sealer(keys("k1", SECRET)).seal(&BINDING, b"c").unwrap();
        assert!(sealer(keys("k1", OTHER)).open(&BINDING, &token).is_err());
    }
}
