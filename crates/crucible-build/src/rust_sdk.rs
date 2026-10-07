use std::path::Path;

use crucible_codegen::{Client, RuntimeDependency, RustOptions};

use crate::{Artifact, ModelPackage};

/// Native Cargo package settings for a named Rust SDK variant.
pub struct RustSdk<'a> {
    model: &'a ModelPackage,
    name: String,
    options: RustOptions,
}

impl<'a> RustSdk<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        Self {
            model,
            name: name.into(),
            options: RustOptions::default(),
        }
    }

    /// Cargo package name; defaults to the service-derived SDK name.
    pub fn package(mut self, package: &str) -> Self {
        self.options.package_name = Some(package.into());
        self
    }

    /// Native SDK version, independent of the producer package's version.
    pub fn version(mut self, version: &str) -> Self {
        self.options.package_version = version.into();
        self
    }

    /// Pin the runtime revision and declare its native Cargo version requirement.
    pub fn runtime_git(
        mut self,
        repository: &str,
        revision: &str,
        version_requirement: &str,
    ) -> Self {
        self.options.runtime = RuntimeDependency::Git {
            repository: repository.into(),
            revision: revision.into(),
            version_requirement: version_requirement.into(),
        };
        self
    }

    /// Use a local runtime crate, relative to the producer's Cargo.toml.
    pub fn runtime_path(mut self, path: impl AsRef<Path>) -> Self {
        self.options.runtime = RuntimeDependency::Path(crate::absolute(path.as_ref()));
        self
    }

    /// Generate and publish the named Cargo source package under OUT_DIR.
    pub fn generate(self) -> Artifact {
        self.model
            .sdk(&self.name, Client::Rust(self.options))
            .generate()
    }

    /// Fallible equivalent of generation.
    pub fn try_generate(self) -> Result<Artifact, String> {
        self.model
            .sdk(&self.name, Client::Rust(self.options))
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
            .sdk(&self.name, Client::Rust(self.options))
            .try_generate_into(output)
    }
}
