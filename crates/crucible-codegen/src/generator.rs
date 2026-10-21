use crucible_ir::Diagnostics;
use crucible_ir::ir::Service;

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
    #[cfg(feature = "json-schema")]
    JsonSchema(crate::JsonSchemaOptions),
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
            #[cfg(feature = "json-schema")]
            Self::JsonSchema(_) => "json-schema",
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
            projected = crucible_ir::project(service, audience)?;
            &projected
        };
        match self {
            Self::Client(client) => {
                client.emit_projected(service, audience, &crate::provenance::Stamp::default())
            }
            Self::Server(server) => server.emit(service),
            #[cfg(feature = "docs")]
            Self::Docs => docs_projected(service, &crate::DocsOptions::default()),
            #[cfg(feature = "openapi")]
            Self::OpenApi(options) => {
                crucible_codegen_openapi::generate(service, options).map(|contents| {
                    vec![File {
                        path: "openapi.json".into(),
                        contents,
                    }]
                })
            }
            #[cfg(feature = "json-schema")]
            Self::JsonSchema(options) => crucible_codegen_jsonschema::generate(service, options)
                .map(|contents| {
                    vec![File {
                        path: "schema.json".into(),
                        contents,
                    }]
                }),
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

/// The API reference [`Generator::Docs`] emits, for `audience`, with
/// `options` (where `describe.json` carries the operations' schemas).
#[cfg(feature = "docs")]
pub fn docs(
    service: &Service,
    audience: crate::Audience,
    options: &crate::DocsOptions,
) -> Result<Vec<File>, Diagnostics> {
    docs_projected(&crucible_ir::project(service, audience)?, options)
}

/// Every supported release's reference in one output, each in its own
/// directory with an index beside them
/// ([`crucible_codegen_docs::generate_releases`]): `service` is the
/// current model (or a line of it), each release's line projected to
/// `audience` (release first, audience second).
#[cfg(feature = "docs")]
pub fn docs_releases(
    service: &Service,
    audience: crate::Audience,
    options: &crate::DocsOptions,
) -> Result<Vec<File>, Diagnostics> {
    crucible_codegen_docs::generate_releases(service, audience, options).map(|files| {
        files
            .into_iter()
            .map(|file| File {
                path: file.path,
                contents: file.contents,
            })
            .collect()
    })
}

#[cfg(feature = "docs")]
fn docs_projected(
    service: &Service,
    options: &crate::DocsOptions,
) -> Result<Vec<File>, Diagnostics> {
    crucible_codegen_docs::generate_with(service, options).map(|files| {
        files
            .into_iter()
            .map(|file| File {
                path: file.path,
                contents: file.contents,
            })
            .collect()
    })
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
