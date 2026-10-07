use crucible_codegen::DartOptions;

use crate::ModelPackage;

/// Native Dart package settings for a named SDK variant.
pub struct DartSdk<'a> {
    model: &'a ModelPackage,
    name: String,
    options: DartOptions,
}

impl<'a> DartSdk<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        Self {
            model,
            name: name.into(),
            options: DartOptions {
                package_name: format!("{}_sdk", crate::sdk_builder::native_snake(name)),
                package_version: std::env::var("CARGO_PKG_VERSION")
                    .unwrap_or_else(|_| "0.1.0".into()),
                endpoint: None,
                content_sha256: false,
            },
        }
    }

    /// Replace all native generator options for advanced configuration.
    pub fn options(mut self, options: DartOptions) -> Self {
        self.options = options;
        self
    }

    /// Dart package identifier.
    pub fn package(mut self, package: &str) -> Self {
        self.options.package_name = package.into();
        self
    }

    /// Native SDK version, independent of the model producer's version.
    pub fn version(mut self, version: &str) -> Self {
        self.options.package_version = version.into();
        self
    }

    /// Default endpoint embedded in the generated client package.
    pub fn endpoint(mut self, endpoint: &str) -> Self {
        self.options.endpoint = Some(endpoint.into());
        self
    }

    /// Enable the existing content SHA-256 client option.
    pub fn content_sha256(mut self, enabled: bool) -> Self {
        self.options.content_sha256 = enabled;
        self
    }

    crate::sdk_builder::generation_methods!(Dart);
}
