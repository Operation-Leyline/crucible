use crucible_codegen::{Client, JavaOptions};

use crate::{Artifact, ModelPackage};

/// Native Maven package settings for a named Java SDK variant.
pub struct JavaSdk<'a> {
    model: &'a ModelPackage,
    name: String,
    options: JavaOptions,
}

impl<'a> JavaSdk<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        Self {
            model,
            name: name.into(),
            options: JavaOptions {
                group_id: "example".into(),
                artifact_id: format!("{name}-sdk"),
                package_name: "example.sdk".into(),
                package_version: std::env::var("CARGO_PKG_VERSION")
                    .unwrap_or_else(|_| "0.1.0".into()),
                endpoint: None,
                content_sha256: false,
            },
        }
    }

    /// Java package containing models and the service-named client.
    pub fn package(mut self, package: &str) -> Self {
        self.options.package_name = package.into();
        self
    }
    /// Maven group ID.
    pub fn group_id(mut self, group: &str) -> Self {
        self.options.group_id = group.into();
        self
    }
    /// Maven artifact ID.
    pub fn artifact_id(mut self, artifact: &str) -> Self {
        self.options.artifact_id = artifact.into();
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
    /// Generate and publish the named native source package.
    pub fn generate(self) -> Artifact {
        self.model
            .sdk(&self.name, Client::Java(self.options))
            .generate()
    }
    /// Fallible equivalent of generation.
    pub fn try_generate(self) -> Result<Artifact, String> {
        self.model
            .sdk(&self.name, Client::Java(self.options))
            .try_generate()
    }
}
