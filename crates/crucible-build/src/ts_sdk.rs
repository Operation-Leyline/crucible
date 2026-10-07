use crucible_codegen::{TsMiddleware, TsOptions};

use crate::ModelPackage;

/// Native npm package settings for a named TypeScript SDK variant.
pub struct TsSdk<'a> {
    model: &'a ModelPackage,
    name: String,
    options: TsOptions,
}

impl<'a> TsSdk<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        Self {
            model,
            name: name.into(),
            options: TsOptions {
                package_name: format!("{name}-sdk"),
                package_version: std::env::var("CARGO_PKG_VERSION")
                    .unwrap_or_else(|_| "0.1.0".into()),
                client_name: None,
                endpoint: None,
                middleware: Vec::new(),
                package_json: None,
            },
        }
    }

    /// Replace all native generator options for advanced configuration.
    pub fn options(mut self, options: TsOptions) -> Self {
        self.options = options;
        self
    }

    /// npm package name, including an optional scope.
    pub fn package(mut self, package: &str) -> Self {
        self.options.package_name = package.into();
        self
    }

    /// Native SDK version, independent of the model producer's version.
    pub fn version(mut self, version: &str) -> Self {
        self.options.package_version = version.into();
        self
    }

    /// Override the default service-named TypeScript client.
    pub fn client_name(mut self, name: &str) -> Self {
        self.options.client_name = Some(name.into());
        self
    }

    /// Default endpoint embedded in the generated client package.
    pub fn endpoint(mut self, endpoint: &str) -> Self {
        self.options.endpoint = Some(endpoint.into());
        self
    }

    /// Replace the middleware every generated client runs by default.
    pub fn middleware(mut self, middleware: Vec<TsMiddleware>) -> Self {
        self.options.middleware = middleware;
        self
    }

    /// Additional metadata merged into the generated package.json.
    pub fn package_json(mut self, metadata: serde_json::Value) -> Self {
        self.options.package_json = Some(metadata);
        self
    }

    crate::sdk_builder::generation_methods!(Ts);
}
