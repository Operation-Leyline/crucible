use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod export_artifact;
mod output;
mod policy;
mod scaffold;
mod sdk;
mod smithy_input;
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
    /// A Smithy source directory, .smithy file, compiled JSON AST, or a
    /// Crucible language package (a directory whose crucible.toml has [model]).
    model: PathBuf,
    /// The service to lower, when the model defines several.
    #[arg(long)]
    service: Option<ShapeId>,
}

#[derive(Subcommand)]
enum Command {
    /// Export a named artifact from a Cargo model package.
    ExportArtifact(export_artifact::Args),
    /// Create a single-crate Hello service or Rust client with Smithy generation.
    New {
        /// Cargo package name, for example hello-service.
        name: String,
        /// Create a Rust client library with a runnable Hello example.
        #[arg(long)]
        client: bool,
        /// Destination directory; defaults to the package name. Must be empty.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Override this CLI's matching Crucible source revision.
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
        /// Run project policy rules in addition to the supported-subset check.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Compare an old contract with a proposed one, reporting compatibility findings.
    Diff {
        /// Baseline Smithy source directory or assembled JSON model.
        old: PathBuf,
        /// Proposed Smithy source directory or assembled JSON model.
        new: PathBuf,
        #[arg(long)]
        service: Option<ShapeId>,
        #[arg(long)]
        config: Option<PathBuf>,
        /// Print a machine-readable report, including excepted findings.
        #[arg(long)]
        json: bool,
    },
    // The compiler helper emits Cargo directives. Isolate assembly so diff's
    // stdout remains a JSON report when used by CI.
    #[command(name = "_assemble-model", hide = true)]
    AssembleModel {
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Print the tested extended Smithy capabilities of each client as JSON.
    Capabilities,
    /// Generate code for one target.
    Generate {
        /// Public outputs omit @internal API unless explicitly selected.
        #[arg(long, global = true, value_enum, default_value = "external")]
        audience: AudienceArg,
        #[command(subcommand)]
        target: Box<Target>,
    },
    /// Freeze a `.cru` package's release as a snapshot of its compiled
    /// model, `releases/<version>.json`, recorded in `releases/crucible.lock`.
    /// `[model.baseline]` points at the latest; every later build fails on a
    /// breaking change against it.
    Freeze {
        /// The package directory, holding crucible.toml with [model].
        package: PathBuf,
        /// The releases directory. Defaults to `releases` inside the package.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Freeze the package as it was at this git commit or tag, to backfill
        /// a release that was never frozen. Compiled with this compiler.
        #[arg(long, value_name = "GIT_REF")]
        from: Option<String>,
    },
    /// Compile a `.cru` package and print the lowered model, with its custom
    /// metadata and documentation, for tools that read the contract (policy
    /// scripts, build steps) without linking the compiler.
    Compile {
        /// The package directory, holding crucible.toml with [model].
        package: PathBuf,
        /// The service to lower, when the package declares several.
        #[arg(long)]
        service: Option<ShapeId>,
        /// Print the model as JSON: the release snapshot format, in full.
        #[arg(long, required = true)]
        json: bool,
        /// Write the JSON to this file instead of standard output.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Work with the SDKs a service declares in crucible-sdk.toml.
    Sdk(sdk::SdkArgs),
    /// Write a stub for every operation that has no file yet: one file per
    /// operation, in a folder per resource (`service/` for the operations the
    /// service binds), under `--out`. Existing files are never overwritten.
    Scaffold {
        #[command(flatten)]
        source: Source,
        /// The operations folder.
        #[arg(long, default_value = "src/operations")]
        out: PathBuf,
        /// Override the derived service namespace (default: crate::<service_name>).
        #[arg(long)]
        contract: Option<String>,
        /// The type that implements every operation.
        #[arg(long, default_value = "crate::operations::Service")]
        service_type: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum AudienceArg {
    External,
    Internal,
}

impl From<AudienceArg> for crucible_codegen::Audience {
    fn from(value: AudienceArg) -> Self {
        match value {
            AudienceArg::External => Self::External,
            AudienceArg::Internal => Self::Internal,
        }
    }
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
    /// A Markdown API reference in mdBook layout, with describe.json.
    Docs {
        #[command(flatten)]
        source: Source,
        /// Output directory; only previously generated files are replaced.
        #[arg(long)]
        out: PathBuf,
    },
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
        /// SDK output directory. Only previously generated files are replaced.
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
        /// A JSON file merged over the generated package.json. It cannot set
        /// the name, version, crucible, files, private or publishConfig.
        #[arg(long)]
        package_json: Option<PathBuf>,
        #[command(flatten)]
        metadata: MetadataArgs,
        /// The npm registry to publish to: package.json `publishConfig.registry`.
        #[arg(long)]
        npm_registry: Option<String>,
        /// `publishConfig.access`: public or restricted.
        #[arg(long, requires = "npm_registry", value_parser = ["public", "restricted"])]
        npm_access: Option<String>,
    },
    /// Rust server bindings as one file, or a complete Cargo SDK with --client.
    Rust {
        #[command(flatten)]
        source: Source,
        /// Server output file, or client Cargo package directory.
        #[arg(long)]
        out: PathBuf,
        /// The client instead of the server side.
        #[arg(long)]
        client: bool,
        /// Cargo SDK package name; defaults to the service name followed by -sdk.
        #[arg(long, requires = "client")]
        package_name: Option<String>,
        /// Cargo SDK package version; defaults to the generator version.
        #[arg(long, requires = "client")]
        package_version: Option<String>,
        /// Local Rust client runtime crate for development.
        #[arg(long, requires = "client")]
        runtime_path: Option<PathBuf>,
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
        #[command(flatten)]
        metadata: MetadataArgs,
    },
}

/// The common package metadata keys, for an ad hoc package
/// (docs/sdk-packaging.md, "The common keys").
#[derive(Args)]
struct MetadataArgs {
    /// One paragraph describing the package; defaults to a generated one.
    #[arg(long)]
    description: Option<String>,
    /// The source repository's URL.
    #[arg(long)]
    repository: Option<String>,
    /// The project's home page URL.
    #[arg(long)]
    homepage: Option<String>,
    /// The documentation URL, where the ecosystem has a field for it.
    #[arg(long)]
    documentation: Option<String>,
    /// An SPDX license expression.
    #[arg(long, conflicts_with = "license_file")]
    license: Option<String>,
    /// A license file, copied into the package as LICENSE.
    #[arg(long)]
    license_file: Option<PathBuf>,
    /// `Name <email>`; repeat for more. The first is the maintainer.
    #[arg(long = "author", value_name = "AUTHOR")]
    authors: Vec<String>,
    /// A Markdown file, copied into the package as README.md.
    #[arg(long)]
    readme: Option<PathBuf>,
    /// A registry keyword; repeat for more.
    #[arg(long = "keyword", value_name = "KEYWORD")]
    keywords: Vec<String>,
    /// Never publish to a public registry: the manifest carries the guard.
    #[arg(long)]
    private: bool,
}

impl MetadataArgs {
    fn is_empty(&self) -> bool {
        self.description.is_none()
            && self.repository.is_none()
            && self.homepage.is_none()
            && self.documentation.is_none()
            && self.license.is_none()
            && self.license_file.is_none()
            && self.authors.is_empty()
            && self.readme.is_none()
            && self.keywords.is_empty()
            && !self.private
    }

    /// The metadata, with the README and license read as LF text.
    fn read(self) -> Result<crucible_codegen::PackageMetadata, String> {
        let read = |path: Option<PathBuf>| {
            path.map(|path| {
                std::fs::read_to_string(&path)
                    .map(|text| crucible_codegen::metadata::normalize_text(&text))
                    .map_err(|error| format!("cannot read {}: {error}", path.display()))
            })
            .transpose()
        };
        let mut metadata = crucible_codegen::PackageMetadata::new();
        metadata.description = self.description;
        metadata.repository = self.repository;
        metadata.homepage = self.homepage;
        metadata.documentation = self.documentation;
        metadata.license = self.license;
        metadata.license_file = read(self.license_file)?;
        metadata.authors = self.authors;
        metadata.readme = read(self.readme)?;
        metadata.keywords = self.keywords;
        metadata.private = self.private;
        Ok(metadata)
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum MiddlewareArg {
    /// `x-amz-content-sha256` on every request with a body.
    ContentSha256,
}

fn main() -> ExitCode {
    use clap::{CommandFactory, FromArgMatches};
    // The version names the revision too: it is what `crucible sdk`'s
    // revision check compares with a service's Cargo.lock.
    let version: &'static str = Box::leak(version().into_boxed_str());
    let matches = Cli::command().version(version).get_matches();
    match Cli::from_arg_matches(&matches) {
        Ok(cli) => run(cli.command),
        Err(error) => error.exit(),
    }
}

/// `X.Y.Z (<revision>)`, `X.Y.Z (<revision>+dirty)` from a checkout with
/// uncommitted changes, or `X.Y.Z` without a verified Git revision.
fn version() -> String {
    match crucible_revision::recorded_revision() {
        Some(revision) => format!("{} ({revision})", env!("CARGO_PKG_VERSION")),
        None => env!("CARGO_PKG_VERSION").into(),
    }
}

// Keep parsing separate from dispatch: their debug-build stack frames together
// can exceed Windows' main-thread stack as the CLI's target portfolio grows.
fn run(command: Command) -> ExitCode {
    let result = match command {
        Command::ExportArtifact(args) => export_artifact::run(&args),
        Command::New {
            name,
            client,
            out,
            crucible_rev,
            runtime_path,
            smithy_version,
        } => {
            let out = out.unwrap_or_else(|| PathBuf::from(&name));
            starter::create(&name, &out, crucible_rev.as_deref(), runtime_path.as_deref(), smithy_version.as_deref(), client)
                .map(|()| {
                    if client {
                        println!("Created {}. Build with: cd {} && cargo build; call a Hello service with: cargo run --example hello -- <endpoint>", name, out.display());
                    } else {
                        println!("Created {}. Open {} in RustRover and Run, or: cd {} && cargo run", name, out.display(), out.display());
                    }
                })
        }
        Command::Check {
            source,
            target,
            config,
        } => lower_with_capabilities(
            &source,
            &target.map(ClientLanguage::capabilities).unwrap_or_default(),
        )
        .and_then(|service| {
            // A package's manifest may carry its policy; --config selects another.
            let manifest = is_cru_package(&source.model)
                .then(|| source.model.join("crucible.toml"))
                .filter(|path| {
                    std::fs::read_to_string(path)
                        .is_ok_and(|text| crucible_policy::Config::declares_policy(&text))
                });
            if let Some(path) = config.or(manifest) {
                policy::check(&service, &path)?;
            }
            print!("{}", summary(&service));
            Ok(())
        }),
        Command::Diff {
            old,
            new,
            service,
            config,
            json,
        } => policy::diff(&old, &new, service.as_ref(), config.as_deref(), json),
        Command::AssembleModel { source, out } => smithy_input::assemble(&source, &out),
        Command::Capabilities => {
            let capabilities: BTreeMap<_, _> = ClientLanguage::ALL
                .into_iter()
                .map(|target| {
                    (
                        target
                            .to_possible_value()
                            .expect("every language has a CLI name")
                            .get_name()
                            .to_owned(),
                        target
                            .capabilities()
                            .iter()
                            .map(|feature| feature.name())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&capabilities).expect("capabilities are JSON strings")
            );
            Ok(())
        }
        Command::Generate { target, audience } => generate(*target, audience.into()),
        Command::Freeze { package, out, from } => freeze(&package, out.as_deref(), from.as_deref()),
        Command::Compile {
            package,
            service,
            json: _,
            out,
        } => compile(package, service, out.as_deref()),
        Command::Sdk(args) => sdk::run(args),
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
    if is_cru_package(&source.model) {
        let service = lower_cru(source)?;
        service
            .require_capabilities(capabilities)
            .map_err(|d| d.to_string())?;
        return Ok(service);
    }
    let model = load_model(source)?;
    model
        .lower_with_capabilities(source.service.as_ref(), capabilities)
        .map_err(|error| error.to_string())
}

fn lower_for_audience(
    source: &Source,
    audience: crucible_codegen::Audience,
    capabilities: &crucible_smithy::Capabilities,
) -> Result<Service, String> {
    let all = crucible_smithy::Capabilities::new(crucible_smithy::Feature::ALL.iter().copied());
    let full = lower_with_capabilities(source, &all)?;
    let projected = crucible_smithy::project(&full, audience).map_err(|d| d.to_string())?;
    projected
        .require_capabilities(capabilities)
        .map_err(|d| d.to_string())?;
    Ok(projected)
}

fn lower_infrastructure(source: &Source) -> Result<Service, String> {
    load_model(source)?
        .lower_infrastructure(source.service.as_ref())
        .map_err(|error| error.to_string())
}

/// Whether `path` is a Crucible language package rather than Smithy sources.
fn is_cru_package(path: &Path) -> bool {
    path.is_dir() && crucible_lang::is_package(path)
}

/// Compiles a language package straight to the IR, as `crucible-build` does:
/// no Smithy compiler and no JVM.
fn lower_cru(source: &Source) -> Result<Service, String> {
    let path = &source.model;
    let package = crucible_lang::load_package(path)?;
    if let Some(id) = &source.service
        && id.namespace() != package.namespace
    {
        return Err(format!(
            "{}: the package namespace is {}, so it has no service {id}",
            path.display(),
            package.namespace
        ));
    }
    let name = source.service.as_ref().map(ShapeId::name);
    let compiled = crucible_lang::compile(&package, name);
    let rendered = compiled.render();
    let Some(service) = compiled.service.clone() else {
        return Err(format!("{} does not compile:\n{rendered}", path.display()));
    };
    // Warnings and advice do not stop generation; show them all the same.
    for line in rendered
        .lines()
        .filter(|line| line.contains(": warning: ") || line.contains(": advice: "))
    {
        eprintln!("{line}");
    }
    // The compatibility guard: a breaking change against the package's
    // frozen baseline that no migration declares fails, as it does in a build.
    let manifest = path.join("crucible.toml");
    let config = std::fs::read_to_string(&manifest)
        .ok()
        .filter(|text| crucible_policy::Config::declares_policy(text))
        .map(|_| crucible_policy::Config::load(&manifest))
        .transpose()?
        .unwrap_or_default();
    if let Some(guard) = crucible_lang::guard(&package, &compiled, package.files.len(), &config) {
        let guard = guard?;
        let rendered = guard.render(&compiled);
        if guard.has_errors() {
            return Err(format!(
                "{} breaks compatibility with its baseline {}:\n{rendered}",
                path.display(),
                guard.version
            ));
        }
        eprint!("{rendered}");
    }
    Ok(service)
}

/// `crucible compile --json`: the package's lowered model as the release
/// snapshot format, documentation and custom metadata included.
fn compile(package: PathBuf, service: Option<ShapeId>, out: Option<&Path>) -> Result<(), String> {
    if !is_cru_package(&package) {
        return Err(format!(
            "{} is not a Crucible language package (a directory whose crucible.toml has [model])",
            package.display()
        ));
    }
    let service = lower_cru(&Source {
        model: package,
        service,
    })?;
    let compiler = format!("crucible {}", env!("CARGO_PKG_VERSION"));
    let text = crucible_smithy::snapshot::write(&service, &compiler);
    match out {
        Some(path) => std::fs::write(path, text)
            .map_err(|error| format!("cannot write {}: {error}", path.display())),
        None => {
            print!("{text}");
            Ok(())
        }
    }
}

/// `crucible freeze`: writes the package's release, its compiled model, to
/// `releases/<version>.json` and records it in the lock. With `from`, the
/// package as it was at that git commit or tag.
fn freeze(package: &Path, out: Option<&Path>, from: Option<&str>) -> Result<(), String> {
    let checkout;
    let source = match from {
        Some(reference) => {
            checkout = package_at(package, reference)?;
            // Freezing needs no baseline, and one written for an older
            // layout, or not in the tree at that commit, would not load.
            let manifest = checkout.path().join("crucible.toml");
            if let Ok(text) = std::fs::read_to_string(&manifest) {
                let mut table: toml::Table = text
                    .parse()
                    .map_err(|error| format!("crucible.toml at {reference}: {error}"))?;
                if let Some(model) = table.get_mut("model").and_then(toml::Value::as_table_mut) {
                    model.remove("baseline");
                }
                let kept = toml::to_string(&table).map_err(|error| error.to_string())?;
                std::fs::write(&manifest, kept).map_err(|error| error.to_string())?;
            }
            checkout.path().to_path_buf()
        }
        None => package.to_path_buf(),
    };
    let loaded = crucible_lang::load_package(&source)?;
    let compiled = crucible_lang::compile(&loaded, None);
    let service = compiled.service.as_ref().ok_or_else(|| {
        format!(
            "{} does not compile:\n{}",
            from.map_or_else(
                || package.display().to_string(),
                |reference| format!("{} at {reference}", package.display())
            ),
            compiled.render()
        )
    })?;
    let releases = out.map_or_else(|| package.join("releases"), Path::to_path_buf);
    let compiler = format!("crucible {}", env!("CARGO_PKG_VERSION"));
    let path = crucible_lang::release::freeze(&releases, service, &compiler)?;
    println!(
        "froze {} {} into {}; point [model.baseline] path at it",
        loaded.namespace,
        service.version,
        path.display()
    );
    Ok(())
}

/// The package directory as it was at `reference`, checked out from its git
/// repository into a temporary directory: only the package's own files.
fn package_at(package: &Path, reference: &str) -> Result<tempfile::TempDir, String> {
    if reference.starts_with('-') {
        return Err(format!("`{reference}` is not a git commit or tag"));
    }
    let git = |args: &[&str]| -> Result<Vec<u8>, String> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(package)
            .args(args)
            .output()
            .map_err(|error| format!("cannot run git: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(output.stdout)
    };
    // The package's path inside its repository, as git names it.
    let prefix = String::from_utf8_lossy(&git(&["rev-parse", "--show-prefix"])?)
        .trim()
        .to_owned();
    let commit = String::from_utf8_lossy(&git(&[
        "rev-parse",
        "--verify",
        &format!("{reference}^{{commit}}"),
    ])?)
    .trim()
    .to_owned();
    // Full names, from the repository root: `git show` takes those.
    let listed = git(&[
        "ls-tree",
        "-r",
        "-z",
        "--full-name",
        "--name-only",
        &commit,
        "--",
        ".",
    ])?;
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let mut copied = 0;
    for name in listed
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name = String::from_utf8_lossy(name).into_owned();
        let Some(relative) = name.strip_prefix(&prefix) else {
            continue;
        };
        // What the compiler reads: manifests and model sources. Not older
        // releases, generated output or submodules.
        if !(relative.ends_with(".cru") || relative.ends_with("crucible.toml")) {
            continue;
        }
        let contents = git(&["show", &format!("{commit}:{name}")])?;
        let target = directory.path().join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&target, contents).map_err(|error| error.to_string())?;
        copied += 1;
    }
    if copied == 0 {
        return Err(format!("{} has no files at {reference}", package.display()));
    }
    Ok(directory)
}

fn load_model(source: &Source) -> Result<Model, String> {
    if is_cru_package(&source.model) {
        return Err(format!(
            "documentation and infrastructure from a .cru package ({}) are not supported yet: the language does not lower resources for them",
            source.model.display()
        ));
    }
    let workspace;
    let model = if source.model.is_dir()
        || source
            .model
            .extension()
            .is_some_and(|extension| extension == "smithy")
    {
        workspace = tempfile::tempdir()
            .map_err(|error| format!("cannot create Smithy compilation directory: {error}"))?;
        crucible_build::compile_smithy(&source.model, workspace.path())
            .map_err(|error| error.to_string())?
    } else {
        source.model.clone()
    };
    let json = std::fs::read_to_string(&model)
        .map_err(|error| format!("cannot read {}: {error}", model.display()))?;
    Model::from_json(&json).map_err(|error| error.to_string())
}

fn generate(target: Target, audience: crucible_codegen::Audience) -> Result<(), String> {
    match target {
        Target::Docs { source, out } => {
            let full = load_model(&source)?
                .lower_documentation(source.service.as_ref())
                .map_err(|e| e.to_string())?;
            let service = crucible_smithy::project(&full, audience).map_err(|e| e.to_string())?;
            let files = crucible_codegen_docs::generate(&service).map_err(|e| e.to_string())?;
            let files = files
                .into_iter()
                .map(|file| crucible_codegen::File {
                    path: file.path,
                    contents: file.contents,
                })
                .collect::<Vec<_>>();
            crucible_codegen::write(&out, &files).map_err(|e| e.to_string())?;
            println!("wrote API reference to {}", out.display());
            Ok(())
        }
        Target::Openapi {
            source,
            out,
            endpoint,
        } => {
            let service =
                lower_for_audience(&source, audience, &crucible_codegen_openapi::capabilities())?;
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
            let service = crucible_smithy::project(&lower_infrastructure(&source)?, audience)
                .map_err(|d| d.to_string())?;
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
            let service = crucible_smithy::project(&lower_infrastructure(&source)?, audience)
                .map_err(|d| d.to_string())?;
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
            let service =
                lower_for_audience(&source, audience, &crucible_smithy::Capabilities::default())?;
            let runtime_path = runtime_path.canonicalize().map_err(|error| {
                format!(
                    "cannot resolve runtime path {}: {error}",
                    runtime_path.display()
                )
            })?;
            for name in ["crucible-client-runtime-rust", "crucible-cli-runtime"] {
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
            let service =
                lower_for_audience(&source, audience, &crucible_codegen_ruby::capabilities())?;
            let client = crucible_codegen::Client::Ruby(crucible_codegen::RubyOptions {
                package_name,
                module_name,
                package_version,
                endpoint,
                content_sha256,
            });
            let files = client
                .emit_for_audience(&service, audience)
                .map_err(|d| format!("cannot generate the Ruby client:\n{d}"))?;
            crucible_codegen::write(&out, &files)
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
            let service =
                lower_for_audience(&source, audience, &crucible_codegen_c::capabilities())?;
            let client = crucible_codegen::Client::C(crucible_codegen::COptions {
                package_name,
                prefix,
                package_version,
                endpoint,
                content_sha256,
            });
            let files = client
                .emit_for_audience(&service, audience)
                .map_err(|d| format!("cannot generate the C client:\n{d}"))?;
            crucible_codegen::write(&out, &files)
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
            let service =
                lower_for_audience(&source, audience, &crucible_codegen_r::capabilities())?;
            let client = crucible_codegen::Client::R(crucible_codegen::ROptions {
                package_name,
                package_version,
                endpoint,
                content_sha256,
            });
            let files = client
                .emit_for_audience(&service, audience)
                .map_err(|d| format!("cannot generate the R client:\n{d}"))?;
            crucible_codegen::write(&out, &files)
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
            let service =
                lower_for_audience(&source, audience, &crucible_codegen_cpp::capabilities())?;
            let client = crucible_codegen::Client::Cpp(crucible_codegen::CppOptions {
                package_name,
                namespace,
                package_version,
                endpoint,
                content_sha256,
            });
            let files = client
                .emit_for_audience(&service, audience)
                .map_err(|d| format!("cannot generate the C++ client:\n{d}"))?;
            crucible_codegen::write(&out, &files)
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
            let service =
                lower_for_audience(&source, audience, &crucible_codegen_php::capabilities())?;
            let client = crucible_codegen::Client::Php(crucible_codegen::PhpOptions {
                package_name,
                namespace,
                package_version,
                endpoint,
                content_sha256,
            });
            let files = client
                .emit_for_audience(&service, audience)
                .map_err(|d| format!("cannot generate the PHP client:\n{d}"))?;
            crucible_codegen::write(&out, &files)
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
            let service =
                lower_for_audience(&source, audience, &crucible_codegen_dart::capabilities())?;
            let client = crucible_codegen::Client::Dart(crucible_codegen::DartOptions {
                package_name,
                package_version,
                endpoint,
                content_sha256,
            });
            let files = client
                .emit_for_audience(&service, audience)
                .map_err(|d| format!("cannot generate the Dart client:\n{d}"))?;
            crucible_codegen::write(&out, &files)
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
                lower_for_audience(&source, audience, &crucible_codegen_swift::capabilities())?;
            let client = crucible_codegen::Client::Swift(crucible_codegen::SwiftOptions {
                package_name,
                module_name,
                package_version,
                endpoint,
                content_sha256,
            });
            let files = client
                .emit_for_audience(&service, audience)
                .map_err(|d| format!("cannot generate the Swift client:\n{d}"))?;
            crucible_codegen::write(&out, &files)
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
                lower_for_audience(&source, audience, &crucible_codegen_csharp::capabilities())?;
            let client = crucible_codegen::Client::Csharp(crucible_codegen::CsharpOptions {
                namespace,
                package_id,
                package_version,
                endpoint,
                content_sha256,
            });
            let files = client
                .emit_for_audience(&service, audience)
                .map_err(|d| format!("cannot generate the C# client:\n{d}"))?;
            crucible_codegen::write(&out, &files)
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
                lower_for_audience(&source, audience, &crucible_codegen_kotlin::capabilities())?;
            let client = crucible_codegen::Client::Kotlin(crucible_codegen::KotlinOptions {
                group_id,
                artifact_id,
                package_name,
                package_version,
                endpoint,
                content_sha256,
            });
            let files = client
                .emit_for_audience(&service, audience)
                .map_err(|d| format!("cannot generate the Kotlin client:\n{d}"))?;
            crucible_codegen::write(&out, &files)
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
            let service =
                lower_for_audience(&source, audience, &crucible_codegen_java::capabilities())?;
            let client = crucible_codegen::Client::Java(crucible_codegen::JavaOptions {
                group_id,
                artifact_id,
                package_name,
                package_version,
                endpoint,
                content_sha256,
            });
            let files = client
                .emit_for_audience(&service, audience)
                .map_err(|d| format!("cannot generate the Java client:\n{d}"))?;
            crucible_codegen::write(&out, &files)
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
            let service =
                lower_for_audience(&source, audience, &crucible_codegen_go::capabilities())?;
            let client = crucible_codegen::Client::Go(crucible_codegen::GoOptions {
                module_path,
                package_name,
                endpoint,
                content_sha256,
            });
            let files = client
                .emit_for_audience(&service, audience)
                .map_err(|d| format!("cannot generate the Go client:\n{d}"))?;
            crucible_codegen::write(&out, &files)
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
                lower_for_audience(&source, audience, &crucible_codegen_python::capabilities())?;
            let client = crucible_codegen::Client::Python(crucible_codegen::PythonOptions {
                package_name,
                module_name,
                package_version,
                endpoint,
                content_sha256,
            });
            let files = client
                .emit_for_audience(&service, audience)
                .map_err(|d| format!("cannot generate the Python client:\n{d}"))?;
            crucible_codegen::write(&out, &files)
                .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            println!("wrote {} files to {}", files.len(), out.display());
            Ok(())
        }
        Target::Rust {
            source,
            out,
            client,
            package_name,
            package_version,
            runtime_path,
            validation_error,
            internal_error,
            unknown_operation_error,
            strict_inputs,
            metadata,
        } => {
            if !client && !metadata.is_empty() {
                return Err("package metadata describes a client package: add --client".into());
            }
            let capabilities = if client {
                crucible_codegen_rust::client_capabilities()
            } else {
                crucible_codegen_rust::capabilities()
            };
            let service = if client {
                lower_for_audience(&source, audience, &capabilities)?
            } else {
                lower_with_capabilities(&source, &capabilities)?
            };
            if client {
                let mut options = crucible_codegen::RustOptions {
                    package_name,
                    metadata: metadata.read()?,
                    ..Default::default()
                };
                if let Some(version) = package_version {
                    options.package_version = version;
                }
                if let Some(path) = runtime_path {
                    options.runtime = crucible_codegen::RuntimeDependency::Path(
                        path.canonicalize()
                            .map_err(|e| format!("cannot locate {}: {e}", path.display()))?,
                    );
                }
                let files = crucible_codegen::Client::Rust(options)
                    .emit_for_audience(&service, audience)
                    .map_err(|d| format!("cannot generate the Rust SDK:\n{d}"))?;
                crucible_codegen::write(&out, &files)
                    .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
                println!("wrote {} files to {}", files.len(), out.display());
                return Ok(());
            }
            let options = crucible_codegen_rust::Options {
                validation_error,
                internal_error,
                unknown_operation_error,
                strict_inputs,
                ..Default::default()
            };
            let code = crucible_codegen_rust::generate(&service, &options)
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
            metadata,
            npm_registry,
            npm_access,
        } => {
            let service =
                lower_for_audience(&source, audience, &crucible_codegen_ts::capabilities())?;
            let package_json = package_json
                .map(|path| {
                    let text = std::fs::read_to_string(&path)
                        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
                    serde_json::from_str(&text)
                        .map_err(|error| format!("{} is not JSON: {error}", path.display()))
                })
                .transpose()?;
            let options = crucible_codegen::TsOptions {
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
                metadata: {
                    let mut metadata = metadata.read()?;
                    metadata.registry = npm_registry
                        .map(|url| crucible_codegen::Registry::npm(url, npm_access.as_deref()));
                    metadata
                },
            };
            let files = crucible_codegen::Client::Ts(options)
                .emit_for_audience(&service, audience)
                .map_err(|diagnostics| {
                    format!(
                        "cannot generate the TypeScript client:
{diagnostics}"
                    )
                })?;
            crucible_codegen::write(&out, &files)
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
            Shape::Unit { .. } => "unit",
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
    out.push_str(&permissions_summary(service));
    let custom = custom_traits(service);
    if !custom.is_empty() {
        out.push_str("  custom traits:\n");
        for (id, uses) in custom {
            out.push_str(&format!("    {id} ×{uses}\n"));
        }
    }
    out
}

/// The permission registry's size and how operations declare requirements;
/// every pending operation is listed with its reason (docs/permissions.md
/// §4.7). Empty for a service that says nothing about authorization.
fn permissions_summary(service: &Service) -> String {
    use crucible_smithy::ir::Requirements;
    let declared = service
        .operations
        .iter()
        .filter(|op| !op.requirements.is_undeclared())
        .count();
    if service.permissions.is_empty() && declared == 0 {
        return String::new();
    }
    let count = |test: fn(&Requirements) -> bool| {
        service
            .operations
            .iter()
            .filter(|op| test(&op.requirements))
            .count()
    };
    let mut out = format!(
        "  {} permissions on {} resources; {} operations require some, {} none, {} pending\n",
        service
            .permissions
            .values()
            .map(|r| r.permissions.len())
            .sum::<usize>(),
        service.permissions.len(),
        count(|r| matches!(r, Requirements::List(_))),
        count(|r| matches!(r, Requirements::None)),
        count(|r| matches!(r, Requirements::Pending { .. })),
    );
    for op in &service.operations {
        if let Requirements::Pending { reason } = &op.requirements {
            out.push_str(&format!("  pending {}: {reason}\n", op.id));
        }
    }
    out
}

/// Traits from the model's own namespaces, which only a project plugin reads.
fn custom_traits(service: &Service) -> BTreeMap<&ShapeId, usize> {
    let standard = |id: &ShapeId| {
        id.is_prelude() || id.namespace().starts_with("aws.") || id.namespace() == "smithy.test"
    };
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

#[cfg(test)]
mod summary_tests {
    #[test]
    fn the_summary_counts_permissions_and_lists_pending_operations() {
        let source = r#"
service Shop {
    version "1"
    protocol json
    auth bearer
    operations [Ping]
    resources [Store]
}
readonly operation Ping {
    http GET "/ping"
    requires none
}
string StoreId { pattern "^s_" }
resource Store {
    id storeId: StoreId
    permission read = "store:read"
    @permissionOnly
    permission audit = "store:audit"
}
readonly operation GetStore on Store {
    http GET "/stores/{storeId}"
    requires read
    input { $storeId }
}
operation Restore on Store {
    http POST "/stores/{storeId}/restore"
    requires pending because "the handler checks"
    input { $storeId }
}
"#;
        let mut package = crucible_lang::Package::new(
            "example.shop",
            vec![crucible_lang::SourceFile {
                path: "shop.cru".into(),
                text: source.into(),
            }],
        );
        package.imports = vec!["crucible.errors".to_owned()];
        let compiled = crucible_lang::compile(&package, None);
        let service = compiled
            .service
            .clone()
            .unwrap_or_else(|| panic!("{}", compiled.render()));
        let summary = super::summary(&service);
        assert!(
            summary.contains(
                "  2 permissions on 1 resources; 1 operations require some, 1 none, 1 pending\n  pending example.shop#Restore: the handler checks\n"
            ),
            "{summary}"
        );
        let weather = crucible_smithy::Model::from_json(include_str!(
            "../../crucible-smithy/tests/fixtures/weather.json"
        ))
        .unwrap()
        .lower(None)
        .unwrap();
        assert!(!super::summary(&weather).contains("permissions"));
    }

    #[test]
    fn custom_trait_counts_use_the_multiplication_sign() {
        let service = crucible_smithy::Model::from_json(include_str!(
            "../../crucible-smithy/tests/fixtures/weather.json"
        ))
        .unwrap()
        .lower(None)
        .unwrap();
        let report = super::summary(&service);
        assert!(report.contains(" \u{00d7}"), "{report}");
    }
}
