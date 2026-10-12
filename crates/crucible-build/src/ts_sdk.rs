use crucible_codegen::{TsMiddleware, TsOptions};

use crate::ModelPackage;

/// Native npm package settings for a named TypeScript SDK variant.
pub struct TsSdk<'a> {
    model: &'a ModelPackage,
    name: String,
    audience: crucible_codegen::Audience,
    options: TsOptions,
    files: crate::package_metadata::MetadataFiles,
}

impl<'a> TsSdk<'a> {
    /// Select internal API explicitly; the default is external.
    pub fn audience(mut self, audience: crucible_codegen::Audience) -> Self {
        self.audience = audience;
        self
    }

    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        Self {
            model,
            name: name.into(),
            audience: crucible_codegen::Audience::External,
            options: TsOptions {
                package_name: format!("{name}-sdk"),
                package_version: std::env::var("CARGO_PKG_VERSION")
                    .unwrap_or_else(|_| "0.1.0".into()),
                client_name: None,
                endpoint: None,
                middleware: Vec::new(),
                package_json: None,
                metadata: crucible_codegen::PackageMetadata::default(),
            },
            files: crate::package_metadata::MetadataFiles::default(),
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

    /// Additional metadata merged into the generated package.json, last. It
    /// cannot set the name, version, `crucible`, `files`, `private` or
    /// `publishConfig`.
    pub fn package_json(mut self, metadata: serde_json::Value) -> Self {
        self.options.package_json = Some(metadata);
        self
    }

    crate::package_metadata::metadata_setters!();

    /// The output this builder configures, with its metadata files read.
    fn output(mut self) -> Result<crate::Output<'a>, String> {
        self.files.apply(&mut self.options.metadata)?;
        Ok(self
            .model
            .sdk(&self.name, crucible_codegen::Client::Ts(self.options))
            .audience(self.audience))
    }

    /// Generate and publish the named native SDK under OUT_DIR.
    pub fn generate(self) -> crate::Artifact {
        self.try_generate()
            .unwrap_or_else(|message| panic!("crucible: {message}"))
    }

    /// Generate without panicking when the target reports diagnostics.
    pub fn try_generate(self) -> Result<crate::Artifact, String> {
        self.output()?.try_generate()
    }

    /// Generate under OUT_DIR and export relative to the producer's manifest.
    pub fn generate_into(self, output: impl AsRef<std::path::Path>) -> crate::Artifact {
        self.try_generate_into(output)
            .unwrap_or_else(|message| panic!("crucible: {message}"))
    }

    /// Fallible equivalent of native SDK generation and export.
    pub fn try_generate_into(
        self,
        output: impl AsRef<std::path::Path>,
    ) -> Result<crate::Artifact, String> {
        self.output()?.try_generate_into(output)
    }
}
