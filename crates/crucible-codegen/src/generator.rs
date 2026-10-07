use crucible_smithy::Diagnostics;
use crucible_smithy::ir::Service;

use crate::{Client, File};

/// A model output and its native configuration, independent of the build system.
#[allow(
    clippy::large_enum_variant,
    reason = "Keep native option structs directly constructible across feature combinations"
)]
pub enum Generator {
    Client(Client),
    #[cfg(feature = "openapi")]
    OpenApi(crate::OpenApiOptions),
    #[cfg(feature = "cdk")]
    Cdk {
        bindings: crate::ProvisioningBindings,
        options: crate::CdkOptions,
    },
    #[cfg(feature = "terraform")]
    Terraform {
        bindings: crate::ProvisioningBindings,
        options: crate::TerraformOptions,
    },
    #[cfg(feature = "cli")]
    Cli(crate::CliOptions),
}

impl Generator {
    /// Stable output identifier used by artifact registries and build adapters.
    pub fn language(&self) -> &'static str {
        match self {
            Self::Client(client) => client.language(),
            #[cfg(feature = "openapi")]
            Self::OpenApi(_) => "openapi",
            #[cfg(feature = "cdk")]
            Self::Cdk { .. } => "cdk",
            #[cfg(feature = "terraform")]
            Self::Terraform { .. } => "terraform",
            #[cfg(feature = "cli")]
            Self::Cli(_) => "cli",
        }
    }

    /// Generate files from the lowered model without changing an output directory.
    pub fn emit(&self, service: &Service) -> Result<Vec<File>, Diagnostics> {
        match self {
            Self::Client(client) => client.emit(service),
            #[cfg(feature = "openapi")]
            Self::OpenApi(options) => {
                crucible_codegen_openapi::generate(service, options).map(|contents| {
                    vec![File {
                        path: "openapi.json".into(),
                        contents,
                    }]
                })
            }
            #[cfg(feature = "cdk")]
            Self::Cdk { bindings, options } => {
                let plan = crucible_codegen_infra::resolve(service, bindings)?;
                crucible_codegen_cdk::emit(service, &plan, options).map(|files| {
                    files
                        .into_iter()
                        .map(|file| File {
                            path: file.path,
                            contents: file.contents,
                        })
                        .collect()
                })
            }
            #[cfg(feature = "terraform")]
            Self::Terraform { bindings, options } => {
                let plan = crucible_codegen_infra::resolve(service, bindings)?;
                crucible_codegen_terraform::emit(service, &plan, options).map(|files| {
                    files
                        .into_iter()
                        .map(|file| File {
                            path: file.path,
                            contents: file.contents,
                        })
                        .collect()
                })
            }
            #[cfg(feature = "cli")]
            Self::Cli(options) => crucible_codegen_cli::emit(service, options).map(|files| {
                files
                    .into_iter()
                    .map(|file| File {
                        path: file.path,
                        contents: file.contents,
                    })
                    .collect()
            }),
        }
    }
}

impl From<Client> for Generator {
    fn from(client: Client) -> Self {
        Self::Client(client)
    }
}
