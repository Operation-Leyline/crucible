use crucible_codegen::GoOptions;

use crate::ModelPackage;

/// Native Go module settings for a named SDK variant.
pub struct GoSdk<'a> {
    model: &'a ModelPackage,
    name: String,
    options: GoOptions,
}

impl<'a> GoSdk<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        Self {
            model,
            name: name.into(),
            options: GoOptions {
                module_path: format!("example.com/{name}-sdk"),
                package_name: format!("{}_sdk", crate::output_builder::native_snake(name)),
                endpoint: None,
                content_sha256: false,
            },
        }
    }

    /// Replace all native generator options for advanced configuration.
    pub fn options(mut self, options: GoOptions) -> Self {
        self.options = options;
        self
    }

    /// Go package identifier, independent of the module path.
    pub fn package(mut self, package: &str) -> Self {
        self.options.package_name = package.into();
        self
    }

    /// Go module import path, including a major-version suffix when required.
    pub fn module_path(mut self, module: &str) -> Self {
        self.options.module_path = module.into();
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

    crate::output_builder::generation_methods!(Go);
}
