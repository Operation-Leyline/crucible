use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

mod output;
mod scaffold;

use clap::{Args, Parser, Subcommand, ValueEnum};
use crucible_smithy::ir::{Service, Shape, Traits};
use crucible_smithy::{Model, ShapeId};

/// Generate servers and clients from a Smithy model.
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
}

#[derive(Subcommand)]
enum Command {
    /// Load a model and report every construct crucible cannot generate from.
    Check {
        #[command(flatten)]
        source: Source,
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

#[derive(Subcommand)]
enum Target {
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
    /// A Kotlin JVM/Android package with suspend operations and Flow pagination.
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
        Command::Check { source } => lower(&source).map(|service| print!("{}", summary(&service))),
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
    let json = std::fs::read_to_string(&source.model)
        .map_err(|error| format!("cannot read {}: {error}", source.model.display()))?;
    let model = Model::from_json(&json).map_err(|error| error.to_string())?;
    model
        .lower(source.service.as_ref())
        .map_err(|error| error.to_string())
}

fn generate(target: Target) -> Result<(), String> {
    match target {
        Target::Csharp {
            source,
            out,
            namespace,
            package_id,
            package_version,
            endpoint,
            content_sha256,
        } => {
            let service = lower(&source)?;
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
            let service = lower(&source)?;
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
            let service = lower(&source)?;
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
            let service = lower(&source)?;
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
            let service = lower(&source)?;
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
            strict_inputs,
        } => {
            let service = lower(&source)?;
            let options = crucible_codegen_rust::Options {
                validation_error,
                internal_error,
                strict_inputs,
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
            let service = lower(&source)?;
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

fn summary(service: &Service) -> String {
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for shape in service.shapes.values() {
        let kind = match shape {
            Shape::Unit => "unit",
            Shape::Simple(_) => "simple",
            Shape::Enum(_) => "enum",
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
