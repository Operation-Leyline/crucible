//! What a crucible server and a crucible client agree on about the wire,
//! with no HTTP stack: operation descriptions, the error model, validation,
//! timestamps, blobs, and the JSON readers and writers both sides use. The
//! `crucible` and `crucible-client-runtime-rust` crates re-export it at the same paths,
//! so generated model code compiles against either.

pub mod blob;
pub mod bound;
pub mod cbor;
pub mod codec;
mod cors_info;
pub mod error;
pub mod event_stream;
pub mod field;
pub mod number;
pub mod open;
pub mod operation;
pub mod split;
pub mod stream;
pub mod text;
pub mod time;
pub mod validate;
