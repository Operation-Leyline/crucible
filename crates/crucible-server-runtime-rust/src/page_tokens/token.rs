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

/// The largest cursor a token carries: room for a composite database key
/// (DynamoDB's `LastEvaluatedKey` of a table and an index), small enough
/// that its token still fits a query string.
pub(crate) const MAX_CURSOR: usize = 4096;

/// The longest token [`Sealer::seal`] writes, and so the longest
/// [`Sealer::open`] reads: anything longer is refused before it is decoded.
const MAX_TOKEN: usize = token_length(super::page_keys::MAX_ID, MAX_CURSOR);

/// The length of a token sealed under a key whose id is `key_id` characters
/// long around a cursor of `cursor` bytes: `p1.<id>.<base64url>`, the payload
/// being nonce, expiry, cursor and tag in unpadded base64 (four characters per
/// three bytes, rounded up).
pub(crate) const fn token_length(key_id: usize, cursor: usize) -> usize {
    VERSION.len() + 1 + key_id + 1 + ((NONCE + EXPIRY + cursor + TAG) * 4).div_ceil(3)
}

/// The largest cursor, at most [`MAX_CURSOR`], whose token under a key id of
/// `key_id` characters is at most `token` characters. `None` when not even a
/// one-byte cursor fits.
pub(crate) fn cursor_capacity(key_id: usize, token: usize) -> Option<usize> {
    // Inverts `token_length`: n bytes take ceil(4n/3) characters, so e
    // characters hold floor(3e/4) bytes.
    let payload = token.checked_sub(VERSION.len() + 2 + key_id)?;
    let cursor = (payload / 4 * 3 + payload % 4 * 3 / 4).checked_sub(NONCE + EXPIRY + TAG)?;
    (cursor >= 1).then_some(cursor.min(MAX_CURSOR))
}

pub(crate) type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// What every token a service issues is sealed with.
#[derive(Clone)]
pub(crate) struct Sealer {
    pub(crate) keys: PageKeys,
    pub(crate) ttl: Duration,
    pub(crate) clock: Clock,
    /// The longest cursor `seal` accepts, at most [`MAX_CURSOR`].
    pub(crate) max_cursor: usize,
}

/// A token that did not open. Deliberately without a reason: every case has
/// the same remedy, and the reason would tell a forger which check failed.
#[derive(Debug)]
pub(crate) struct Invalid;

impl Sealer {
    /// The longest token `seal` writes under the current key.
    pub(crate) fn longest_token(&self) -> usize {
        token_length(self.keys.current().id.len(), self.max_cursor)
    }

    /// `None` when the cursor is longer than `max_cursor`.
    pub(crate) fn seal(&self, binding: &[u8; 32], cursor: &[u8]) -> Option<String> {
        if cursor.len() > self.max_cursor {
            return None;
        }
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
        if token.len() > MAX_TOKEN {
            return Err(Invalid);
        }
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
            max_cursor: MAX_CURSOR,
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
    fn the_longest_token_is_what_the_formula_says() {
        for id in ["k", "key-0123456789ab"] {
            for max_cursor in [1, 100, 1483, MAX_CURSOR] {
                let mut sealer = sealer(keys(id, SECRET));
                sealer.max_cursor = max_cursor;
                let token = sealer.seal(&BINDING, &vec![b'x'; max_cursor]).unwrap();
                assert_eq!(token.len(), sealer.longest_token(), "{id} {max_cursor}");
                assert!(sealer.seal(&BINDING, &vec![b'x'; max_cursor + 1]).is_none());
            }
        }
    }

    #[test]
    fn the_capacity_is_the_largest_cursor_that_fits() {
        for id in [1, 16] {
            for limit in [60, 200, 2048, 5000, 8000] {
                match cursor_capacity(id, limit) {
                    Some(cursor) => {
                        assert!(token_length(id, cursor) <= limit, "{id} {limit}");
                        assert!(
                            cursor == MAX_CURSOR || token_length(id, cursor + 1) > limit,
                            "{id} {limit}"
                        );
                    }
                    None => assert!(token_length(id, 1) > limit, "{id} {limit}"),
                }
            }
        }
        assert_eq!(cursor_capacity(2, 0), None);
        assert_eq!(cursor_capacity(2, usize::MAX), Some(MAX_CURSOR));
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
            max_cursor: MAX_CURSOR,
        };
        let token = sealer.seal(&BINDING, b"c").unwrap();
        now.store(1_000_060, Ordering::Relaxed);
        assert!(sealer.open(&BINDING, &token).is_ok());
        now.store(1_000_061, Ordering::Relaxed);
        assert!(sealer.open(&BINDING, &token).is_err());
    }

    #[test]
    fn the_longest_cursor_seals_and_a_longer_one_does_not() {
        let sealer = sealer(keys("sixteen-chars-id", SECRET));
        let longest = vec![b'x'; MAX_CURSOR];
        let token = sealer.seal(&BINDING, &longest).unwrap();
        // The cap on what `open` reads is exactly what `seal` writes.
        assert_eq!(token.len(), MAX_TOKEN);
        assert_eq!(sealer.open(&BINDING, &token).unwrap(), longest);
        assert!(sealer.seal(&BINDING, &[b'x'; MAX_CURSOR + 1]).is_none());
    }

    #[test]
    fn an_overlong_token_is_refused_before_it_is_read() {
        let sealer = sealer(keys("k1", SECRET));
        let token = sealer.seal(&BINDING, b"c").unwrap();
        let padded = format!("{token}{}", "A".repeat(MAX_TOKEN));
        assert!(sealer.open(&BINDING, &padded).is_err());
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
