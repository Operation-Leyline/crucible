use crucible_codegen::COptions;

use crate::ModelPackage;

/// Native CMake package settings for a named C SDK variant.
pub struct CSdk<'a> {
    model: &'a ModelPackage,
    name: String,
    audience: crucible_codegen::Audience,
    options: COptions,
}

impl<'a> CSdk<'a> {
    /// Select internal API explicitly; the default is external.
    pub fn audience(mut self, audience: crucible_codegen::Audience) -> Self {
        self.audience = audience;
        self
    }

    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        let native_name = format!("{}_sdk", crate::output_builder::native_snake(name));
        Self {
            model,
            name: name.into(),
            audience: crucible_codegen::Audience::External,
            options: COptions {
                package_name: native_name.clone(),
                prefix: native_name,
                package_version: std::env::var("CARGO_PKG_VERSION")
                    .unwrap_or_else(|_| "0.1.0".into()),
                endpoint: None,
                content_sha256: false,
            },
        }
    }

    /// Replace the complete native generator configuration.
    pub fn options(mut self, options: COptions) -> Self {
        self.options = options;
        self
    }

    /// Native CMake package name.
    pub fn package(mut self, package: &str) -> Self {
        self.options.package_name = package.into();
        self
    }

    /// C symbol prefix containing generated models and the service client.
    pub fn prefix(mut self, prefix: &str) -> Self {
        self.options.prefix = prefix.into();
        self
    }

    /// Native SDK version, independent of the model producer package's version.
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

    crate::output_builder::generation_methods!(C);
}
