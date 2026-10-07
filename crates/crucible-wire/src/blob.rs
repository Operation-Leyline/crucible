//! Smithy `blob`: bytes. Inside a JSON body a blob is base64 (the standard
//! alphabet, padded); as an `@httpPayload` it is the whole HTTP body, raw.
//!
//! [`Blob`] holds [`Bytes`], so a payload read from a request or response
//! body shares that body's buffer instead of copying it.

use std::cell::Cell;
use std::fmt;
use std::ops::Deref;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use serde::de::{self, Deserializer, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

/// The bytes of a Smithy `blob`.
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Blob(Bytes);

impl Blob {
    pub fn new(bytes: impl Into<Bytes>) -> Self {
        Self(bytes.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Bytes {
        self.0
    }

    /// The bytes as base64, as a JSON body carries them.
    pub fn to_base64(&self) -> String {
        STANDARD.encode(&self.0)
    }

    /// Bytes from base64 text, or `None` when the text is not base64.
    pub fn from_base64(text: &str) -> Option<Self> {
        STANDARD.decode(text).ok().map(Self::new)
    }
}

impl fmt::Debug for Blob {
    // A blob can be large and unprintable: its length says enough.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Blob({} bytes)", self.0.len())
    }
}

impl Deref for Blob {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for Blob {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<Bytes> for Blob {
    fn from(bytes: Bytes) -> Self {
        Self(bytes)
    }
}

impl From<Vec<u8>> for Blob {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes.into())
    }
}

impl From<&'static [u8]> for Blob {
    fn from(bytes: &'static [u8]) -> Self {
        Self(Bytes::from_static(bytes))
    }
}

impl<const N: usize> From<&'static [u8; N]> for Blob {
    fn from(bytes: &'static [u8; N]) -> Self {
        Self(Bytes::from_static(bytes))
    }
}

impl From<String> for Blob {
    fn from(text: String) -> Self {
        Self(text.into())
    }
}

impl From<&'static str> for Blob {
    fn from(text: &'static str) -> Self {
        Self(Bytes::from_static(text.as_bytes()))
    }
}

impl From<Blob> for Bytes {
    fn from(blob: Blob) -> Self {
        blob.0
    }
}

impl Serialize for Blob {
    /// Base64 text for JSON; bytes for a format that has them, which is how
    /// a payload writer takes them raw.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_base64())
        } else {
            serializer.serialize_bytes(&self.0)
        }
    }
}

/// The newtype name a blob asks its deserializer for. The bound-member
/// reader (`bound.rs`) answers it by handing over a whole body's `Bytes`,
/// which serde's visitor methods cannot carry, through [`HANDOFF`].
pub(crate) const TOKEN: &str = "$crucible_server_runtime_rust::private::Blob";

thread_local! {
    /// Bytes on their way from the bound-member reader to the `Blob` it is
    /// building. Set and taken within one synchronous `deserialize` call.
    static HANDOFF: Cell<Option<Bytes>> = const { Cell::new(None) };
}

/// Runs `visit` with `bytes` waiting for the blob it builds: a visitor that
/// is a blob's takes them without copying, any other leaves them, and they
/// are dropped once `visit` returns.
pub(crate) fn hand_off<R>(bytes: Bytes, visit: impl FnOnce() -> R) -> R {
    HANDOFF.set(Some(bytes));
    let result = visit();
    HANDOFF.take();
    result
}

impl<'de> Deserialize<'de> for Blob {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_newtype_struct(TOKEN, BlobVisitor)
    }
}

struct BlobVisitor;

impl<'de> Visitor<'de> for BlobVisitor {
    type Value = Blob;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("base64 text")
    }

    /// What every deserializer but the bound-member reader does with the
    /// newtype: the value itself, base64 text in JSON.
    fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<Blob, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Blob, E> {
        Blob::from_base64(text).ok_or_else(|| E::custom("must be base64"))
    }

    fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<Blob, E> {
        Ok(Blob(Bytes::copy_from_slice(bytes)))
    }

    fn visit_byte_buf<E: de::Error>(self, bytes: Vec<u8>) -> Result<Blob, E> {
        Ok(Blob(bytes.into()))
    }

    /// The bound-member reader's answer to [`TOKEN`]: the bytes are waiting.
    fn visit_unit<E: de::Error>(self) -> Result<Blob, E> {
        HANDOFF
            .take()
            .map(Blob)
            .ok_or_else(|| E::invalid_type(de::Unexpected::Unit, &self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_carries_a_blob_as_padded_standard_base64() {
        let blob = Blob::from(&[0xff, 0x00, 0xfe, b'?', b'>'][..]);
        let json = serde_json::to_string(&blob).unwrap();
        assert_eq!(json, r#""/wD+Pz4=""#);
        assert_eq!(serde_json::from_str::<Blob>(&json).unwrap(), blob);
        let value = serde_json::to_value(&blob).unwrap();
        assert_eq!(serde_json::from_value::<Blob>(value).unwrap(), blob);
    }

    #[test]
    fn text_that_is_not_base64_is_refused() {
        let error = serde_json::from_str::<Blob>(r#""not base64!""#).unwrap_err();
        assert!(error.to_string().contains("must be base64"), "{error}");
        assert!(serde_json::from_str::<Blob>("null").is_err());
        assert!(serde_json::from_str::<Blob>("12").is_err());
    }

    #[test]
    fn debug_shows_the_length_not_the_bytes() {
        assert_eq!(format!("{:?}", Blob::from("secret")), "Blob(6 bytes)");
    }
}
