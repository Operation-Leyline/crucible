use crucible_smithy::Diagnostics;
use crucible_smithy::ir::Service;

use crate::{Client, File, Server};

/// A model output and its native configuration, independent of the build system.
#[allow(
    clippy::large_enum_variant,
    reason = "Keep native option structs directly constructible across feature combinations"
)]
pub enum Generator {
    Client(Client),
    Server(Server),
    #[cfg(feature = "openapi")]
    OpenApi(crate::OpenApiOptions),
    #[cfg(feature = "docs")]
    Docs,
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
    pub fn target(&self) -> &'static str {
        match self {
            Self::Client(client) => client.language(),
            Self::Server(server) => server.target(),
            #[cfg(feature = "openapi")]
            Self::OpenApi(_) => "openapi",
            #[cfg(feature = "docs")]
            Self::Docs => "docs",
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
        self.emit_for_audience(service, crate::Audience::External)
    }

    /// Server bindings always retain the full contract, regardless of audience.
    pub fn emit_for_audience(
        &self,
        service: &Service,
        audience: crate::Audience,
    ) -> Result<Vec<File>, Diagnostics> {
        let projected;
        let service = if matches!(self, Self::Server(_)) {
            service
        } else {
            projected = crucible_smithy::project(service, audience)?;
            &projected
        };
        match self {
            Self::Client(client) => client.emit_projected(service),
            Self::Server(server) => server.emit(service),
            #[cfg(feature = "docs")]
            Self::Docs => crucible_codegen_docs::generate(service).map(|files| {
                files
                    .into_iter()
                    .map(|file| File {
                        path: file.path,
                        contents: file.contents,
                    })
                    .collect()
            }),
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

impl From<Server> for Generator {
    fn from(server: Server) -> Self {
        Self::Server(server)
    }
}
