use crucible_codegen_common::PackageMetadata;

use crate::RuntimeDependency;

/// Native Cargo package configuration for a generated Rust SDK.
#[derive(Debug, Clone)]
pub struct ClientOptions {
    /// Defaults to the service name in kebab case, followed by `-sdk`.
    pub package_name: Option<String>,
    pub package_version: String,
    pub runtime: RuntimeDependency,
    /// Description, URLs, license, authors, README, keywords and `private`.
    pub metadata: PackageMetadata,
    /// The client sends `x-amz-content-sha256`, the SHA-256 of each body,
    /// whatever `Config` it is given (`middleware = ["content-sha256"]`):
    /// a service whose front door signs requests needs it on every write.
    pub content_sha256: bool,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            package_name: None,
            package_version: env!("CARGO_PKG_VERSION").into(),
            runtime: RuntimeDependency::Auto,
            metadata: PackageMetadata::default(),
            content_sha256: false,
        }
    }
}
