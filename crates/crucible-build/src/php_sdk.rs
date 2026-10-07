use crucible_codegen::PhpOptions;

use crate::ModelPackage;

/// Native Composer package settings for a named PHP SDK variant.
pub struct PhpSdk<'a> {
    model: &'a ModelPackage,
    name: String,
    options: PhpOptions,
}

impl<'a> PhpSdk<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        let module = format!("{}Sdk", crate::output_builder::native_pascal(name));
        Self {
            model,
            name: name.into(),
            options: PhpOptions {
                package_name: format!(
                    "example/{}-sdk",
                    name.split('-')
                        .filter(|word| !word.is_empty())
                        .collect::<Vec<_>>()
                        .join("-")
                ),
                namespace: module,
                package_version: std::env::var("CARGO_PKG_VERSION")
                    .unwrap_or_else(|_| "0.1.0".into()),
                endpoint: None,
                content_sha256: false,
            },
        }
    }

    /// Replace the complete native generator configuration.
    pub fn options(mut self, options: PhpOptions) -> Self {
        self.options = options;
        self
    }

    /// Native Composer package name.
    pub fn package(mut self, package: &str) -> Self {
        self.options.package_name = package.into();
        self
    }

    /// PHP namespace containing generated models and the service client.
    pub fn namespace(mut self, namespace: &str) -> Self {
        self.options.namespace = namespace.into();
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

    crate::output_builder::generation_methods!(Php);
}
