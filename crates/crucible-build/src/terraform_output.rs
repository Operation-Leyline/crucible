use std::path::Path;

use crucible_codegen::{Generator, TerraformOptions};

use crate::ModelPackage;
use crate::provisioning_bindings::ProvisioningBindings;

/// Native Terraform/OpenTofu provider settings and explicit lifecycle bindings.
pub struct TerraformOutput<'a> {
    model: &'a ModelPackage,
    name: String,
    options: TerraformOptions,
    bindings: ProvisioningBindings,
}

impl<'a> TerraformOutput<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        Self {
            model,
            name: name.into(),
            options: TerraformOptions {
                module_path: format!("example.com/{name}-provider"),
                provider_name: name.replace('-', "_"),
                provider_address: format!("registry.terraform.io/example/{name}"),
            },
            bindings: ProvisioningBindings::default(),
        }
    }

    /// Replace all native provider generation options.
    pub fn options(mut self, options: TerraformOptions) -> Self {
        self.options = options;
        self
    }

    /// Go module path of the generated provider executable.
    pub fn module_path(mut self, module: &str) -> Self {
        self.options.module_path = module.into();
        self
    }

    /// Terraform provider identifier used by the resource schema.
    pub fn provider_name(mut self, name: &str) -> Self {
        self.options.provider_name = name.into();
        self
    }

    /// Provider address in hostname/namespace/name form.
    pub fn provider_address(mut self, address: &str) -> Self {
        self.options.provider_address = address.into();
        self
    }

    /// Select explicit lifecycle bindings, relative to the producer's Cargo.toml.
    pub fn bindings(mut self, path: impl AsRef<Path>) -> Self {
        self.bindings = ProvisioningBindings::file(path);
        self
    }

    /// Supply already parsed explicit lifecycle bindings.
    pub fn bindings_value(mut self, bindings: crucible_codegen::ProvisioningBindings) -> Self {
        self.bindings = ProvisioningBindings::Value(bindings);
        self
    }

    fn prepare(self) -> Result<(&'a ModelPackage, String, Generator), String> {
        let bindings = self.bindings.load()?;
        Ok((
            self.model,
            self.name,
            Generator::Terraform {
                bindings,
                options: self.options,
            },
        ))
    }

    crate::output_builder::output_generation_methods!();
}
