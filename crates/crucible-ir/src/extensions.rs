//! Typed client behavior descriptions; emitters never interpret raw trait documents.
use serde::Serialize;

/// The one authentication scheme Crucible implements: an `Authorization:
/// Bearer` token. A service lists it as `auth bearer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AuthScheme {
    Bearer,
}

/// The representation of an explicitly bound whole HTTP body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PayloadKind {
    Blob,
    String,
    Document,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HeaderEncoding {
    Base64,
}
