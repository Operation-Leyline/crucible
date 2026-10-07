use std::path::Path;

use crucible_codegen::{CdkOptions, CdkTargets, Generator};

use crate::ModelPackage;
use crate::provisioning_bindings::ProvisioningBindings;

/// Publishing metadata and bindings for a named jsii/CDK construct package.
pub struct CdkOutput<'a> {
    model: &'a ModelPackage,
    name: String,
    options: CdkOptions,
    bindings: ProvisioningBindings,
}

impl<'a> CdkOutput<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        Self {
            model,
            name: name.into(),
            options: CdkOptions {
                package_name: format!("{name}-cdk"),
                package_version: std::env::var("CARGO_PKG_VERSION")
                    .unwrap_or_else(|_| "0.1.0".into()),
                author: std::env::var("CARGO_PKG_AUTHORS").unwrap_or_default(),
                repository_url: std::env::var("CARGO_PKG_REPOSITORY").unwrap_or_default(),
                license: std::env::var("CARGO_PKG_LICENSE").unwrap_or_default(),
                targets: CdkTargets::default(),
            },
            bindings: ProvisioningBindings::default(),
        }
    }

    /// Replace all native CDK publishing options.
    pub fn options(mut self, options: CdkOptions) -> Self {
        self.options = options;
        self
    }

    /// npm package name of the jsii construct library.
    pub fn package(mut self, package: &str) -> Self {
        self.options.package_name = package.into();
        self
    }

    /// Native package version, independent of the producer's version.
    pub fn version(mut self, version: &str) -> Self {
        self.options.package_version = version.into();
        self
    }

    /// Author responsible for publishing this construct library.
    pub fn author(mut self, author: &str) -> Self {
        self.options.author = author.into();
        self
    }

    /// Repository URL for the published package.
    pub fn repository_url(mut self, repository: &str) -> Self {
        self.options.repository_url = repository.into();
        self
    }

    /// License selected by the package publisher.
    pub fn license(mut self, license: &str) -> Self {
        self.options.license = license.into();
        self
    }

    /// jsii publishing targets, with their native package coordinates.
    pub fn targets(mut self, targets: CdkTargets) -> Self {
        self.options.targets = targets;
        self
    }

    /// Read CDK publishing metadata now; later setters can override its fields.
    pub fn config(self, path: impl AsRef<Path>) -> Self {
        self.try_config(path)
            .unwrap_or_else(|message| panic!("crucible: {message}"))
    }

    /// Fallible loading of a required, Cargo-watched CDK metadata file.
    pub fn try_config(mut self, path: impl AsRef<Path>) -> Result<Self, String> {
        let path = crate::absolute(path.as_ref());
        println!("cargo::rerun-if-changed={}", path.display());
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("cannot read CDK config {}: {error}", path.display()))?;
        self.options = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid CDK config {}: {error}", path.display()))?;
        Ok(self)
    }

    /// Select the explicit provisioning bindings file, relative to Cargo.toml.
    pub fn bindings(mut self, path: impl AsRef<Path>) -> Self {
        self.bindings = ProvisioningBindings::file(path);
        self
    }

    /// Supply already parsed explicit provisioning bindings.
    pub fn bindings_value(mut self, bindings: crucible_codegen::ProvisioningBindings) -> Self {
        self.bindings = ProvisioningBindings::Value(bindings);
        self
    }

    fn prepare(self) -> Result<(&'a ModelPackage, String, Generator), String> {
        let bindings = self.bindings.load()?;
        Ok((
            self.model,
            self.name,
            Generator::Cdk {
                bindings,
                options: self.options,
            },
        ))
    }

    crate::output_builder::output_generation_methods!();
}
