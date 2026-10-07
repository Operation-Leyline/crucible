use crate::RuntimeDependency;

/// Native Cargo package configuration for a generated Rust SDK.
#[derive(Debug, Clone)]
pub struct ClientOptions {
    /// Defaults to the service name in kebab case, followed by `-sdk`.
    pub package_name: Option<String>,
    pub package_version: String,
    pub runtime: RuntimeDependency,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            package_name: None,
            package_version: env!("CARGO_PKG_VERSION").into(),
            runtime: RuntimeDependency::Auto,
        }
    }
}
