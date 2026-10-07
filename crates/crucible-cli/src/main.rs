use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

mod output;
mod scaffold;
mod starter;

use clap::{Args, Parser, Subcommand, ValueEnum};
use crucible_smithy::ir::{Service, Shape, Traits};
use crucible_smithy::{Model, ShapeId};

/// Generate servers, clients, and infrastructure packages from a Smithy model.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Source {
    /// A Smithy JSON AST, built with the flattenAndRemoveMixins transform.
    model: PathBuf,
    /// The service to lower, when the model defines several.
    #[arg(long)]
    service: Option<ShapeId>,
    /// Select a declared protocol by trait ID, for example aws.protocols#restJson1.
    #[arg(long)]
    protocol: Option<ShapeId>,
}

#[derive(Subcommand)]
enum Command {
    /// Create a single-crate Hello service that builds and runs immediately.
    New {
        /// Cargo package name, for example hello-service.
        name: String,
        /// Destination directory; defaults to the package name. Must be empty.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Pin a Crucible commit instead of this CLI's matching release tag.
        #[arg(long, conflicts_with = "runtime_path")]
        crucible_rev: Option<String>,
        /// Use a local Crucible checkout for development and qualification.
        #[arg(long)]
        runtime_path: Option<PathBuf>,
        /// Select an upstream Smithy X.Y.Z release instead of the tested default.
        #[arg(long)]
        smithy_version: Option<String>,
    },
    /// Load a model and report every construct crucible cannot generate from.
    Check {
        #[command(flatten)]
        source: Source,
        /// Check the capabilities of a particular generated client.
        #[arg(long, value_enum)]
        target: Option<ClientLanguage>,
    },
    /// Print the tested extended Smithy capabilities of each client as JSON.
    Capabilities,
    /// Export lowered Smithy HTTP protocol tests for a client's native runner.
    Conformance {
        #[command(flatten)]
        source: Source,
        #[arg(long, value_enum)]
        target: ClientLanguage,
        #[arg(long)]
        out: PathBuf,
    },
    /// Generate code for one target.
    Generate {
        #[command(subcommand)]
        target: Target,
    },
    /// Write a stub for every operation that has no file yet: one file per
    /// operation, in a folder per resource (`service/` for the operations the
    /// service binds), under `--out`. Existing files are never overwritten.
    Scaffold {
        #[command(flatten)]
        source: Source,
        /// The operations folder.
        #[arg(long, default_value = "src/operations")]
        out: PathBuf,
        /// The module the generated contract is included in.
        #[arg(long, default_value = "crate::contract")]
        contract: String,
        /// The type that implements every operation.
        #[arg(long, default_value = "crate::operations::Service")]
        service_type: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ClientLanguage {
    Rust,
    Ts,
    Python,
    Go,
    Java,
    Kotlin,
    Csharp,
    Swift,
    Dart,
    Php,
    Ruby,
    Cpp,
    C,
    R,
}

impl ClientLanguage {
    const ALL: [Self; 14] = [
        Self::Rust,
        Self::Ts,
        Self::Python,
        Self::Go,
        Self::Java,
        Self::Kotlin,
        Self::Csharp,
        Self::Swift,
        Self::Dart,
        Self::Php,
        Self::Ruby,
        Self::Cpp,
        Self::C,
        Self::R,
    ];

    fn capabilities(self) -> crucible_smithy::Capabilities {
        match self {
            Self::Rust => crucible_codegen_rust::client_capabilities(),
            Self::Ts => crucible_codegen_ts::capabilities(),
            Self::Python => crucible_codegen_python::capabilities(),
            Self::Go => crucible_codegen_go::capabilities(),
            Self::Java => crucible_codegen_java::capabilities(),
            Self::Kotlin => crucible_codegen_kotlin::capabilities(),
            Self::Csharp => crucible_codegen_csharp::capabilities(),
            Self::Swift => crucible_codegen_swift::capabilities(),
            Self::Dart => crucible_codegen_dart::capabilities(),
            Self::Php => crucible_codegen_php::capabilities(),
            Self::Ruby => crucible_codegen_ruby::capabilities(),
            Self::Cpp => crucible_codegen_cpp::capabilities(),
            Self::C => crucible_codegen_c::capabilities(),
            Self::R => crucible_codegen_r::capabilities(),
        }
    }
}

#[derive(Subcommand)]
enum Target {
    /// An OpenAPI 3.1 JSON document for a REST JSON service.
    Openapi {
        #[command(flatten)]
        source: Source,
        /// The JSON file to write.
        #[arg(long)]
        out: PathBuf,
        /// The API server URL, including any deployment path prefix.
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// A jsii-compatible CDK construct library for explicitly selected resources.
    Cdk {
        #[command(flatten)]
        source: Source,
        /// Provisioning bindings, including the existing provider contract.
        #[arg(long)]
        bindings: PathBuf,
        /// Package metadata and jsii language target configuration.
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// A Terraform/OpenTofu provider for explicitly bound resource lifecycles.
    Terraform {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        bindings: PathBuf,
        #[arg(long)]
        module_path: String,
        #[arg(long)]
        provider_name: String,
        #[arg(long)]
        provider_address: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// A standalone Rust service CLI backed by the generated Rust client.
    Cli {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        bin_name: String,
        #[arg(long, default_value = "0.0.0")]
        package_version: String,
        #[arg(long)]
        endpoint: Option<String>,
        /// Crucible checkout supplying the unpublished client and CLI runtimes.
        #[arg(long)]
        runtime_path: PathBuf,
    },
    /// A Ruby gem with native models and lazy pagination.
    Ruby {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        package_name: String,
        #[arg(long)]
        module_name: String,
        #[arg(long, default_value = "0.0.0")]
        package_version: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        content_sha256: bool,
    },
    /// A native C17 CMake package with arena-owned models and libcurl transport.
    C {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        package_name: String,
        #[arg(long)]
        prefix: String,
        #[arg(long, default_value = "0.0.0")]
        package_version: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        content_sha256: bool,
    },
    /// An R package with S3 models and lazy page iteration.
    R {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        package_name: String,
        #[arg(long, default_value = "0.0.0")]
        package_version: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        content_sha256: bool,
    },
    /// A C++20 CMake package with native models and libcurl transport.
    Cpp {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        package_name: String,
        #[arg(long)]
        namespace: String,
        #[arg(long, default_value = "0.0.0")]
        package_version: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        content_sha256: bool,
    },
    /// A PHP Composer package with typed operations and lazy pagination.
    Php {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        package_name: String,
        #[arg(long)]
        namespace: String,
        #[arg(long, default_value = "0.0.0")]
        package_version: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        content_sha256: bool,
    },
    /// A Dart package with Future operations and Stream pagination.
    Dart {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        package_name: String,
        #[arg(long, default_value = "0.0.0-dev")]
        package_version: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        content_sha256: bool,
    },
    /// A Swift package with native async operations and AsyncSequence pagination.
    Swift {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        package_name: String,
        #[arg(long)]
        module_name: String,
        #[arg(long, default_value = "0.0.0-dev")]
        package_version: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        content_sha256: bool,
    },
    /// A .NET 8 NuGet package with synchronous and asynchronous clients.
    Csharp {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        package_id: String,
        #[arg(long, default_value = "0.0.0-dev")]
        package_version: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        content_sha256: bool,
    },
    /// A Kotlin Multiplatform package with suspend operations and Flow pagination.
    Kotlin {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        group_id: String,
        #[arg(long)]
        artifact_id: String,
        #[arg(long)]
        package_name: String,
        #[arg(long, default_value = "0.0.0-SNAPSHOT")]
        package_version: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        content_sha256: bool,
    },
    /// A Java client package.
    Java {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        group_id: String,
        #[arg(long)]
        artifact_id: String,
        #[arg(long)]
        package_name: String,
        #[arg(long, default_value = "0.0.0-SNAPSHOT")]
        package_version: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        content_sha256: bool,
    },
    /// A Go module with typed models and context-aware HTTP clients.
    Go {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        module_path: String,
        #[arg(long)]
        package_name: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        content_sha256: bool,
    },
    /// A typed Python package with synchronous and asynchronous clients.
    Python {
        #[command(flatten)]
        source: Source,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        package_name: String,
        #[arg(long)]
        module_name: String,
        #[arg(long, default_value = "0.0.0.dev0")]
        package_version: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        content_sha256: bool,
    },
    /// A fetch-based TypeScript client package.
    Ts {
        #[command(flatten)]
        source: Source,
        /// The directory to write the package to. crucible owns it: it is
        /// emptied and rewritten on every run.
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        package_name: String,
        #[arg(long, default_value = "0.0.0-dev")]
        package_version: String,
        /// The client class name; defaults to the service name plus `Client`.
        #[arg(long)]
        client_name: Option<String>,
        /// The endpoint a client uses when its config names none.
        #[arg(long)]
        endpoint: Option<String>,
        /// Middleware every client runs by default.
        #[arg(long, value_enum)]
        middleware: Vec<MiddlewareArg>,
        /// A JSON file merged over the generated package.json.
        #[arg(long)]
        package_json: Option<PathBuf>,
    },
    /// The Rust code a service plugs into the crucible framework, or a
    /// client, as one file: what `crucible-build` writes from a build
    /// script, for reading.
    Rust {
        #[command(flatten)]
        source: Source,
        /// The file to write.
        #[arg(long)]
        out: PathBuf,
        /// The client instead of the server side.
        #[arg(long)]
        client: bool,
        /// The error a rejected request becomes; defaults to the service's
        /// one error named *Validation*.
        #[arg(long)]
        validation_error: Option<ShapeId>,
        /// The error internal failures are answered with; defaults to the
        /// service's one server error with status 500.
        #[arg(long)]
        internal_error: Option<ShapeId>,
        /// The modelled HTTP 404 error for an unmatched route; defaults to
        /// crucible.errors#UnknownOperationException when declared.
        #[arg(long)]
        unknown_operation_error: Option<ShapeId>,
        /// Every structure an input reaches refuses unknown members.
        #[arg(long)]
        strict_inputs: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum MiddlewareArg {
    /// `x-amz-content-sha256` on every request with a body.
    ContentSha256,
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::New { name, out, crucible_rev, runtime_path, smithy_version } => {
            let out = out.unwrap_or_else(|| PathBuf::from(&name));
            starter::create(&name, &out, crucible_rev.as_deref(), runtime_path.as_deref(), smithy_version.as_deref())
                .map(|()| println!("Created {}. Open {} in RustRover and Run, or: cd {} && cargo run", name, out.display(), out.display()))
        },
        Command::Check { source, target } => lower_with_capabilities(
            &source, &target.map(ClientLanguage::capabilities).unwrap_or_default(),
        ).map(|service| print!("{}", summary(&service))),
        Command::Capabilities => {
            let capabilities: BTreeMap<_, _> = ClientLanguage::ALL.into_iter().map(|target| (
                target.to_possible_value().expect("every language has a CLI name").get_name().to_owned(),
                target.capabilities().iter().map(|feature| feature.name()).collect::<Vec<_>>(),
            )).collect();
            println!("{}", serde_json::to_string_pretty(&capabilities).expect("capabilities are JSON strings"));
            Ok(())
        }
        Command::Conformance { source, target, out } => lower_with_capabilities(&source, &target.capabilities()).and_then(|service| {
            let value = serde_json::json!({"schemaVersion":1, "service":service.id, "tests":service.protocol_tests});
            let json = serde_json::to_string_pretty(&value).expect("protocol tests are JSON values");
            output::write(&out, [("protocol-tests.json", json.as_str())])
                .map_err(|error| format!("cannot write {}: {error}", out.display()))?;
            println!("wrote {} protocol tests to {}", service.protocol_tests.len(), out.display());
            Ok(())
        }),
        Command::Generate { target } => generate(target),
        Command::Scaffold {
            source,
            out,
            contract,
            service_type,
        } => lower(&source).and_then(|service| {
            let options = crucible_codegen_rust::ScaffoldOptions {
                contract,
                service_type,
            };
            scaffold::run(&service, &out, &options)
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn lower(source: &Source) -> Result<Service, String> {
    lower_with_capabilities(source, &crucible_smithy::Capabilities::default())
}

fn lower_with_capabilities(
    source: &Source,
    capabilities: &crucible_smithy::Capabilities,
) -> Result<Service, String> {
    let model = load_model(source)?;
    match &source.protocol {
        Some(id) => {
            let protocol = crucible_smithy::Protocol::ALL
                .iter()
                .copied()
                .find(|protocol| protocol.trait_id() == *id)
                .ok_or_else(|| format!("unsupported protocol {id}"))?;
            model.lower_with_protocol(source.service.as_ref(), capabilities, protocol)
        }
        None => model.lower_with_capabilities(source.service.as_ref(), capabilities),
    }
    .map_err(|error| error.to_string())
}

fn lower_infrastructure(source: &Source) -> Result<Service, String> {
    load_model(source)?
        .lower_infrastructure(source.service.as_ref())
        .map_err(|error| error.to_string())
}

fn load_model(source: &Source) -> Result<Model, String> {
    let json = std::fs::read_to_string(&source.model)
        .map_err(|error| format!("cannot read {}: {error}", source.model.display()))?;
    Model::from_json(&json).map_err(|error| error.to_string())
}

fn generate(target: Target) -> Result<(), String> {
    match target {
        Target::Openapi {
            source,
            out,
            endpoint,
        } => {
            let service =
                lower_with_capabilities(&source, &crucible_codegen_openapi::capabilities())?;
            let document = crucible_codegen_openapi::generate(
                &service,
                &crucible_codegen_openapi::Options { endpoint },
            )
            .map_err(|diagnostics| format!("cannot generate OpenAPI:\n{diagnostics}"))?;
            std::fs::write(&out, document)
                .map_err(|error| format!("cannot write {}: {error}", out.display()))?;
            println!("wrote {}", out.display());
            Ok(())
        }
        Target::Cdk {
            source,
            bindings,
            config,
            out,
        } => {
            let service = lower_infrastructure(&source)?;
            let bindings = read_config(&bindings)?;
            let options = read_config(&config)?;
            let plan = crucible_codegen_infra::resolve(&service, &bindings)
                .map_err(|d| format!("invalid provisioning bindings:\n{d}"))?;
            let files = crucible_codegen_cdk::emit(&service, &plan, &options)
                .map_err(|d| format!("cannot generate CDK constructs:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Terraform {
            source,
            bindings,
            module_path,
            provider_name,
            provider_address,
            out,
        } => {
            let service = lower_infrastructure(&source)?;
            let bindings = read_config(&bindings)?;
            let plan = crucible_codegen_infra::resolve(&service, &bindings)
                .map_err(|d| format!("invalid provisioning bindings:\n{d}"))?;
            let options = crucible_codegen_terraform::Options {
                module_path,
                provider_name,
                provider_address,
            };
            let files = crucible_codegen_terraform::emit(&service, &plan, &options)
                .map_err(|d| format!("cannot generate Terraform provider:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Cli {
            source,
            out,
            bin_name,
            package_version,
            endpoint,
            runtime_path,
        } => {
            let service = lower(&source)?;
            let runtime_path = runtime_path.canonicalize().map_err(|error| {
                format!(
                    "cannot resolve runtime path {}: {error}",
                    runtime_path.display()
                )
            })?;
            for name in ["crucible-client", "crucible-cli-runtime"] {
                if !runtime_path
                    .join("crates")
                    .join(name)
                    .join("Cargo.toml")
                    .is_file()
                {
                    return Err(format!(
                        "{} is not a Crucible checkout: missing crates/{name}/Cargo.toml",
                        runtime_path.display()
                    ));
                }
            }
            let files = crucible_codegen_cli::emit(
                &service,
                &crucible_codegen_cli::Options {
                    bin_name,
                    package_version,
                    endpoint,
                    runtime_path,
                },
            )
            .map_err(|diagnostics| format!("cannot generate the service CLI:\n{diagnostics}"))?;
            output::write(
                &out,
                files
                    .iter()
                    .map(|file| (file.path.as_str(), file.contents.as_str())),
            )
            .map_err(|error| format!("cannot write {}: {error}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Ruby {
            source,
            out,
            package_name,
            module_name,
            package_version,
            endpoint,
            content_sha256,
        } => {
            let service = lower_with_capabilities(&source, &crucible_codegen_ruby::capabilities())?;
            let files = crucible_codegen_ruby::emit(
                &service,
                &crucible_codegen_ruby::Options {
                    package_name,
                    module_name,
                    package_version,
                    endpoint,
                    content_sha256,
                },
            )
            .map_err(|d| format!("cannot generate the Ruby client:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::C {
            source,
            out,
            package_name,
            prefix,
            package_version,
            endpoint,
            content_sha256,
        } => {
            let service = lower_with_capabilities(&source, &crucible_codegen_c::capabilities())?;
            let files = crucible_codegen_c::emit(
                &service,
                &crucible_codegen_c::Options {
                    package_name,
                    prefix,
                    package_version,
                    endpoint,
                    content_sha256,
                },
            )
            .map_err(|d| format!("cannot generate the C client:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::R {
            source,
            out,
            package_name,
            package_version,
            endpoint,
            content_sha256,
        } => {
            let service = lower_with_capabilities(&source, &crucible_codegen_r::capabilities())?;
            let files = crucible_codegen_r::emit(
                &service,
                &crucible_codegen_r::Options {
                    package_name,
                    package_version,
                    endpoint,
                    content_sha256,
                },
            )
            .map_err(|d| format!("cannot generate the R client:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Cpp {
            source,
            out,
            package_name,
            namespace,
            package_version,
            endpoint,
            content_sha256,
        } => {
            let service = lower_with_capabilities(&source, &crucible_codegen_cpp::capabilities())?;
            let files = crucible_codegen_cpp::emit(
                &service,
                &crucible_codegen_cpp::Options {
                    package_name,
                    namespace,
                    package_version,
                    endpoint,
                    content_sha256,
                },
            )
            .map_err(|d| format!("cannot generate the C++ client:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Php {
            source,
            out,
            package_name,
            namespace,
            package_version,
            endpoint,
            content_sha256,
        } => {
            let service = lower_with_capabilities(&source, &crucible_codegen_php::capabilities())?;
            let files = crucible_codegen_php::emit(
                &service,
                &crucible_codegen_php::Options {
                    package_name,
                    namespace,
                    package_version,
                    endpoint,
                    content_sha256,
                },
            )
            .map_err(|d| format!("cannot generate the PHP client:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Dart {
            source,
            out,
            package_name,
            package_version,
            endpoint,
            content_sha256,
        } => {
            let service = lower_with_capabilities(&source, &crucible_codegen_dart::capabilities())?;
            let files = crucible_codegen_dart::emit(
                &service,
                &crucible_codegen_dart::Options {
                    package_name,
                    package_version,
                    endpoint,
                    content_sha256,
                },
            )
            .map_err(|d| format!("cannot generate the Dart client:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Swift {
            source,
            out,
            package_name,
            module_name,
            package_version,
            endpoint,
            content_sha256,
        } => {
            let service =
                lower_with_capabilities(&source, &crucible_codegen_swift::capabilities())?;
            let files = crucible_codegen_swift::emit(
                &service,
                &crucible_codegen_swift::Options {
                    package_name,
                    module_name,
                    package_version,
                    endpoint,
                    content_sha256,
                },
            )
            .map_err(|d| format!("cannot generate the Swift client:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Csharp {
            source,
            out,
            namespace,
            package_id,
            package_version,
            endpoint,
            content_sha256,
        } => {
            let service =
                lower_with_capabilities(&source, &crucible_codegen_csharp::capabilities())?;
            let files = crucible_codegen_csharp::emit(
                &service,
                &crucible_codegen_csharp::Options {
                    namespace,
                    package_id,
                    package_version,
                    endpoint,
                    content_sha256,
                },
            )
            .map_err(|d| format!("cannot generate the C# client:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Kotlin {
            source,
            out,
            group_id,
            artifact_id,
            package_name,
            package_version,
            endpoint,
            content_sha256,
        } => {
            let service =
                lower_with_capabilities(&source, &crucible_codegen_kotlin::capabilities())?;
            let files = crucible_codegen_kotlin::emit(
                &service,
                &crucible_codegen_kotlin::Options {
                    group_id,
                    artifact_id,
                    package_name,
                    package_version,
                    endpoint,
                    content_sha256,
                },
            )
            .map_err(|d| format!("cannot generate the Kotlin client:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Java {
            source,
            out,
            group_id,
            artifact_id,
            package_name,
            package_version,
            endpoint,
            content_sha256,
        } => {
            let service = lower_with_capabilities(&source, &crucible_codegen_java::capabilities())?;
            let files = crucible_codegen_java::emit(
                &service,
                &crucible_codegen_java::Options {
                    group_id,
                    artifact_id,
                    package_name,
                    package_version,
                    endpoint,
                    content_sha256,
                },
            )
            .map_err(|d| format!("cannot generate the Java client:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Go {
            source,
            out,
            module_path,
            package_name,
            endpoint,
            content_sha256,
        } => {
            let service = lower_with_capabilities(&source, &crucible_codegen_go::capabilities())?;
            let files = crucible_codegen_go::emit(
                &service,
                &crucible_codegen_go::Options {
                    module_path,
                    package_name,
                    endpoint,
                    content_sha256,
                },
            )
            .map_err(|d| format!("cannot generate the Go client:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Python {
            source,
            out,
            package_name,
            module_name,
            package_version,
            endpoint,
            content_sha256,
        } => {
            let service =
                lower_with_capabilities(&source, &crucible_codegen_python::capabilities())?;
            let files = crucible_codegen_python::emit(
                &service,
                &crucible_codegen_python::Options {
                    package_name,
                    module_name,
                    package_version,
                    endpoint,
                    content_sha256,
                },
            )
            .map_err(|d| format!("cannot generate the Python client:\n{d}"))?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Rust {
            source,
            out,
            client,
            validation_error,
            internal_error,
            unknown_operation_error,
            strict_inputs,
        } => {
            let capabilities = if client {
                crucible_codegen_rust::client_capabilities()
            } else {
                crucible_codegen_rust::capabilities()
            };
            let service = lower_with_capabilities(&source, &capabilities)?;
            let options = crucible_codegen_rust::Options {
                validation_error,
                internal_error,
                unknown_operation_error,
                strict_inputs,
                ..Default::default()
            };
            let code = if client {
                crucible_codegen_rust::generate_client(&service)
            } else {
                crucible_codegen_rust::generate(&service, &options)
            };
            let code = code
                .map_err(|diagnostics| format!("cannot generate the Rust code:\n{diagnostics}"))?;
            std::fs::write(&out, code)
                .map_err(|error| format!("cannot write {}: {error}", out.display()))?;
            println!("wrote {}", out.display());
            Ok(())
        }
        Target::Ts {
            source,
            out,
            package_name,
            package_version,
            client_name,
            endpoint,
            middleware,
            package_json,
        } => {
            let service = lower_with_capabilities(&source, &crucible_codegen_ts::capabilities())?;
            let package_json = package_json
                .map(|path| {
                    let text = std::fs::read_to_string(&path)
                        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
                    serde_json::from_str(&text)
                        .map_err(|error| format!("{} is not JSON: {error}", path.display()))
                })
                .transpose()?;
            let options = crucible_codegen_ts::Options {
                package_name,
                package_version,
                client_name,
                endpoint,
                middleware: middleware
                    .into_iter()
                    .map(|m| match m {
                        MiddlewareArg::ContentSha256 => {
                            crucible_codegen_ts::Middleware::ContentSha256
                        }
                    })
                    .collect(),
                package_json,
            };
            let files = crucible_codegen_ts::emit(&service, &options).map_err(|diagnostics| {
                format!(
                    "cannot generate the TypeScript client:
{diagnostics}"
                )
            })?;
            output::write(
                &out,
                files.iter().map(|f| (f.path.as_str(), f.contents.as_str())),
            )
            .map_err(|error| format!("cannot write {}: {error}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
    }
}

fn read_config<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|e| format!("invalid configuration {}: {e}", path.display()))
}

fn summary(service: &Service) -> String {
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for shape in service.shapes.values() {
        let kind = match shape {
            Shape::Unit => "unit",
            Shape::Simple(_) => "simple",
            Shape::Enum(_) => "enum",
            Shape::IntEnum(_) => "intEnum",
            Shape::List(_) => "list",
            Shape::Map(_) => "map",
            Shape::Structure(_) => "structure",
            Shape::Union(_) => "union",
        };
        *kinds.entry(kind).or_default() += 1;
    }
    let kinds: Vec<String> = kinds
        .iter()
        .map(|(kind, n)| format!("{n} {kind}"))
        .collect();
    let paginated = service
        .operations
        .iter()
        .filter(|op| op.paginated.is_some())
        .count();
    let tokens = service
        .operations
        .iter()
        .filter(|op| op.idempotency_token.is_some())
        .count();

    let mut out = format!(
        "{} {}\n  {} operations ({paginated} paginated, {tokens} with idempotency tokens)\n  {} shapes: {}\n",
        service.id,
        service.version,
        service.operations.len(),
        service.shapes.len(),
        kinds.join(", "),
    );
    let custom = custom_traits(service);
    if !custom.is_empty() {
        out.push_str("  custom traits:\n");
        for (id, uses) in custom {
            out.push_str(&format!("    {id} ×{uses}\n"));
        }
    }
    out
}

/// Traits from the model's own namespaces, which only a project plugin reads.
fn custom_traits(service: &Service) -> BTreeMap<&ShapeId, usize> {
    let standard = |id: &ShapeId| id.is_prelude() || id.namespace().starts_with("aws.");
    let mut maps: Vec<&Traits> = service.operations.iter().map(|op| &op.traits).collect();
    for shape in service.shapes.values() {
        if let Shape::Structure(structure) | Shape::Union(structure) = shape {
            maps.push(&structure.traits);
            maps.extend(structure.members.iter().map(|member| &member.traits));
        }
    }
    let mut uses = BTreeMap::new();
    for id in maps
        .into_iter()
        .flat_map(Traits::keys)
        .filter(|id| !standard(id))
    {
        *uses.entry(id).or_default() += 1;
    }
    uses
}
