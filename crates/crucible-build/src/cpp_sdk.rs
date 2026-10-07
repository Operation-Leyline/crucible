use crucible_codegen::CppOptions;

use crate::ModelPackage;

/// Native CMake package settings for a named C++ SDK variant.
pub struct CppSdk<'a> {
    model: &'a ModelPackage,
    name: String,
    options: CppOptions,
}

impl<'a> CppSdk<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        let native_name = format!("{}_sdk", crate::output_builder::native_snake(name));
        Self {
            model,
            name: name.into(),
            options: CppOptions {
                package_name: native_name.clone(),
                namespace: native_name,
                package_version: std::env::var("CARGO_PKG_VERSION")
                    .unwrap_or_else(|_| "0.1.0".into()),
                endpoint: None,
                content_sha256: false,
            },
        }
    }

    /// Replace the complete native generator configuration.
    pub fn options(mut self, options: CppOptions) -> Self {
        self.options = options;
        self
    }

    /// Native CMake package name.
    pub fn package(mut self, package: &str) -> Self {
        self.options.package_name = package.into();
        self
    }

    /// C++ namespace containing generated models and the service client.
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

    crate::output_builder::generation_methods!(Cpp);
}
