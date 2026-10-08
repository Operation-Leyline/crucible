use crucible_codegen::{Client, JavaOptions};
use std::path::Path;

use crate::{Artifact, ModelPackage};

/// Native Gradle/Maven package settings for a named Java SDK variant.
pub struct JavaSdk<'a> {
    model: &'a ModelPackage,
    name: String,
    audience: crucible_codegen::Audience,
    options: JavaOptions,
}

impl<'a> JavaSdk<'a> {
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

    /// Replace all native generator options for advanced configuration.
    pub fn options(mut self, options: JavaOptions) -> Self {
        self.options = options;
        self
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
            .audience(self.audience)
            .generate()
    }
    /// Fallible equivalent of generation.
    pub fn try_generate(self) -> Result<Artifact, String> {
        self.model
            .sdk(&self.name, Client::Java(self.options))
            .audience(self.audience)
            .try_generate()
    }

    /// Generate under OUT_DIR and export to a stable native package path.
    pub fn generate_into(self, output: impl AsRef<Path>) -> Artifact {
        self.try_generate_into(output)
            .unwrap_or_else(|message| panic!("crucible: {message}"))
    }

    /// Fallible equivalent of native package generation and export.
    pub fn try_generate_into(self, output: impl AsRef<Path>) -> Result<Artifact, String> {
        self.model
            .sdk(&self.name, Client::Java(self.options))
            .audience(self.audience)
            .try_generate_into(output)
    }
}
