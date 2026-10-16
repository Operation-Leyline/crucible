use crucible_codegen::SwiftOptions;

use crate::ModelPackage;

/// Native Swift package settings for a named Swift SDK variant.
pub struct SwiftSdk<'a> {
    model: &'a ModelPackage,
    name: String,
    audience: crucible_codegen::Audience,
    options: SwiftOptions,
    module_explicit: bool,
}

impl<'a> SwiftSdk<'a> {
    /// Select internal API explicitly; the default is external.
    pub fn audience(mut self, audience: crucible_codegen::Audience) -> Self {
        self.audience = audience;
        self
    }

    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        let module = format!("{}Sdk", crate::output_builder::native_pascal(name));
        Self {
            model,
            name: name.into(),
            audience: crucible_codegen::Audience::External,
            options: SwiftOptions {
                metadata: Default::default(),
                package_name: module.clone(),
                module_name: module,
                package_version: std::env::var("CARGO_PKG_VERSION")
                    .unwrap_or_else(|_| "0.1.0".into()),
                endpoint: None,
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

    crate::output_builder::generation_methods!(Swift);
}
