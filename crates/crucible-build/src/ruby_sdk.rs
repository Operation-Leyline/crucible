use crucible_codegen::RubyOptions;

use crate::ModelPackage;

/// Native Ruby gem settings for a named Ruby SDK variant.
pub struct RubySdk<'a> {
    model: &'a ModelPackage,
    name: String,
    options: RubyOptions,
}

impl<'a> RubySdk<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        let native_name = format!("{}_sdk", crate::output_builder::native_snake(name));
        let module = format!("{}Sdk", crate::output_builder::native_pascal(name));
        Self {
            model,
            name: name.into(),
            options: RubyOptions {
                package_name: native_name,
                module_name: module,
                package_version: std::env::var("CARGO_PKG_VERSION")
                    .unwrap_or_else(|_| "0.1.0".into()),
                endpoint: None,
                content_sha256: false,
            },
        }
    }

    /// Replace the complete native generator configuration.
    pub fn options(mut self, options: RubyOptions) -> Self {
        self.options = options;
        self
    }

    /// Native Ruby gem name.
    pub fn package(mut self, package: &str) -> Self {
        self.options.package_name = package.into();
        self
    }

    /// Ruby root module containing generated models and the service client.
    pub fn module_name(mut self, module_name: &str) -> Self {
        self.options.module_name = module_name.into();
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

    crate::output_builder::generation_methods!(Ruby);
}
