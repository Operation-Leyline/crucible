use crucible_codegen::PythonOptions;

use crate::ModelPackage;

/// Native Python distribution settings for a named Python SDK variant.
pub struct PythonSdk<'a> {
    model: &'a ModelPackage,
    name: String,
    audience: crucible_codegen::Audience,
    options: PythonOptions,
}

impl<'a> PythonSdk<'a> {
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
            options: PythonOptions {
                package_name: native_name.replace('_', "-"),
                module_name: native_name,
                package_version: std::env::var("CARGO_PKG_VERSION")
                    .unwrap_or_else(|_| "0.1.0".into()),
                endpoint: None,
            },
        }
    }

    /// Replace the complete native generator configuration.
    pub fn options(mut self, options: PythonOptions) -> Self {
        self.options = options;
        self
    }

    /// Native Python distribution name.
    pub fn package(mut self, package: &str) -> Self {
        self.options.package_name = package.into();
        self
    }

    /// Python import module containing generated models and the service client.
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

    crate::output_builder::generation_methods!(Python);
}
