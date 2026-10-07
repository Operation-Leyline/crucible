use crucible_codegen::SwiftOptions;

use crate::ModelPackage;

/// Native Swift package settings for a named Swift SDK variant.
pub struct SwiftSdk<'a> {
    model: &'a ModelPackage,
    name: String,
    options: SwiftOptions,
    module_explicit: bool,
}

impl<'a> SwiftSdk<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        let module = format!("{}Sdk", crate::sdk_builder::native_pascal(name));
        Self {
            model,
            name: name.into(),
            options: SwiftOptions {
                package_name: module.clone(),
                module_name: module,
                package_version: std::env::var("CARGO_PKG_VERSION")
                    .unwrap_or_else(|_| "0.1.0".into()),
                endpoint: None,
                content_sha256: false,
            },
            module_explicit: false,
        }
    }

    /// Replace the complete native generator configuration.
    pub fn options(mut self, options: SwiftOptions) -> Self {
        self.options = options;
        self.module_explicit = true;
        self
    }

    /// Native Swift package name. Also selects the module until explicitly overridden.
    pub fn package(mut self, package: &str) -> Self {
        self.options.package_name = package.into();
        if !self.module_explicit {
            self.options.module_name = package.into();
        }
        self
    }

    /// Swift module containing generated models and the service client.
    pub fn module_name(mut self, module_name: &str) -> Self {
        self.options.module_name = module_name.into();
        self.module_explicit = true;
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

    crate::sdk_builder::generation_methods!(Swift);
}
