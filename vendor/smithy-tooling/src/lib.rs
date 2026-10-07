//! Smithy model assets pinned to one upstream release.
//!
//! ```
//! assert!(!smithy_tooling::SMITHY_VERSION.is_empty());
//! assert!(smithy_tooling::PRELUDE.contains("namespace smithy.api"));
//! assert!(smithy_tooling::MODEL_JAR.starts_with(b"PK"));
//! ```
//!
//! [`build`] runs the bundled CLI from a Cargo build script. By default its
//! matching Java runtime is provisioned and cached on first use; no separate
//! Java or Smithy CLI installation is required. Reading the assets needs no JVM.

mod compiler;
#[cfg(feature = "managed-runtime")]
mod runtime;

pub use compiler::build;

/// Upstream Smithy version, independent of this crate's Rust API version.
pub const SMITHY_VERSION: &str = include_str!("smithy-version.txt");

/// Official prelude extracted without modification from the model JAR.
pub const PRELUDE: &str = include_str!("prelude.smithy");

/// Complete upstream `software.amazon.smithy:smithy-model` JAR.
pub const MODEL_JAR: &[u8] = include_bytes!("../vendor/smithy-model.jar");

/// SHA-256 of [`MODEL_JAR`], checked against Maven Central during import.
pub const MODEL_JAR_SHA256: &str = include_str!("model-jar-sha256.txt");
