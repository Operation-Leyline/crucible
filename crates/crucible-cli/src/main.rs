use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod export_artifact;
mod output;
mod policy;
mod scaffold;
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
    /// Select a declared protocol by trait ID, for example aws.protocols#restJson1.
    #[arg(long)]
    protocol: Option<ShapeId>,
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
    /// Export lowered Smithy HTTP protocol tests for a client's native runner.
    Conformance {
        #[command(flatten)]
        source: Source,
        #[arg(long, value_enum)]
        target: ClientLanguage,
        #[arg(long)]
        out: PathBuf,
        /// Select the same audience as the generated client under test.
        #[arg(long, value_enum, default_value = "external")]
        audience: AudienceArg,
    },
    /// Generate code for one target.
    Generate {
        /// Public outputs omit @internal API unless explicitly selected.
        #[arg(long, global = true, value_enum, default_value = "external")]
        audience: AudienceArg,
        #[command(subcommand)]
        target: Box<Target>,
    },
    /// Copy a `.cru` package's manifest and sources into a frozen baseline,
    /// such as `releases/2026-09-04`, that `[model.baseline]` points at.
    /// Every later build fails on a breaking change against it.
    Freeze {
        /// The package directory, holding crucible.toml with [model].
        package: PathBuf,
        /// Where the frozen copy goes; must not exist or be empty.
        #[arg(long)]
        out: PathBuf,
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
        /// A JSON file merged over the generated package.json.
        #[arg(long)]
        package_json: Option<PathBuf>,
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
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum MiddlewareArg {
    /// `x-amz-content-sha256` on every request with a body.
    ContentSha256,
}

fn main() -> ExitCode {
    run(Cli::parse().command)
}

// Keep parsing separate from dispatch: their debug-build stack frames together
// can exceed Windows' main-thread stack as the CLI's target portfolio grows.
/// Server-only cases are not client contract, and a native runner may parse `kind` first.
fn client_tests(
    service: &crucible_smithy::ir::Service,
) -> Vec<&crucible_smithy::ir::HttpProtocolTest> {
    let server = Some(crucible_smithy::ir::TestAppliesTo::Server);
    service
        .protocol_tests
        .iter()
        .filter(|test| test.applies_to != server)
        .collect()
}

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
        Command::Conformance {
            source,
            target,
            out,
            audience,
        } => lower_for_audience(&source, audience.into(), &target.capabilities()).and_then(
            |service| {
                let tests = client_tests(&service);
                let value =
                    serde_json::json!({"schemaVersion":1, "service":service.id, "tests":tests});
                let json =
                    serde_json::to_string_pretty(&value).expect("protocol tests are JSON values");
                output::write(&out, [("protocol-tests.json", json.as_str())])
                    .map_err(|error| format!("cannot write {}: {error}", out.display()))?;
                println!("wrote {} protocol tests to {}", tests.len(), out.display());
                Ok(())
            },
        ),
        Command::Generate { target, audience } => generate(*target, audience.into()),
        Command::Freeze { package, out } => freeze(&package, &out),
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
    if source.protocol.is_some() {
        return Err(format!(
            "{}: a .cru package declares its protocol with `protocol ..` in the service; --protocol cannot select one",
            path.display()
        ));
    }
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
    let Some(service) = compiled.service else {
        return Err(format!("{} does not compile:\n{rendered}", path.display()));
    };
    // Warnings do not stop generation; show them all the same.
    for line in rendered.lines().filter(|line| line.contains(": warning: ")) {
        eprintln!("{line}");
    }
    // The compatibility guard: a breaking change against the package's
    // frozen baseline fails, as it does in a build.
    if let Some(baseline) = crucible_lang::compile_baseline(&package, name) {
        let old = baseline?;
        let manifest = path.join("crucible.toml");
        let config = std::fs::read_to_string(&manifest)
            .ok()
            .filter(|text| crucible_policy::Config::declares_policy(text))
            .map(|_| crucible_policy::Config::load(&manifest))
            .transpose()?
            .unwrap_or_default();
        let report = crucible_policy::diff(&old, &service, &config);
        if report.has_errors() {
            return Err(format!(
                "{} breaks compatibility with its baseline:\n{report}",
                path.display()
            ));
        }
        eprint!("{report}");
    }
    Ok(service)
}

/// `crucible freeze`: copies a package's manifest and `.cru` sources into
/// `out`, the frozen baseline the next release is checked against. The
/// copy's own `[model.baseline]` is dropped; a baseline has no baseline.
fn freeze(package: &Path, out: &Path) -> Result<(), String> {
    let loaded = crucible_lang::load_package(package)?;
    if out.exists()
        && std::fs::read_dir(out)
            .map_err(|error| format!("cannot read {}: {error}", out.display()))?
            .next()
            .is_some()
    {
        return Err(format!(
            "{} is not empty; a frozen release is never overwritten",
            out.display()
        ));
    }
    let manifest = std::fs::read_to_string(package.join("crucible.toml"))
        .map_err(|error| format!("cannot read the manifest: {error}"))?;
    // Dropped by key, however it is written: a table, an inline table or a
    // dotted key. The frozen copy keeps every other setting.
    let mut table: toml::Table = manifest
        .parse()
        .map_err(|error| format!("cannot read the manifest: {error}"))?;
    if let Some(model) = table.get_mut("model").and_then(toml::Value::as_table_mut) {
        model.remove("baseline");
    }
    let kept = toml::to_string(&table).map_err(|error| error.to_string())?;
    let write = |path: &Path, text: &str| -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        std::fs::write(path, text)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))
    };
    write(&out.join("crucible.toml"), &kept)?;
    let root = std::fs::canonicalize(package)
        .map_err(|error| format!("cannot resolve {}: {error}", package.display()))?;
    for file in &loaded.files {
        let source = std::fs::canonicalize(&file.path)
            .map_err(|error| format!("cannot resolve {}: {error}", file.path))?;
        let relative = source
            .strip_prefix(&root)
            .map_err(|_| format!("{} is outside the package", file.path))?;
        write(&out.join(relative), &file.text)?;
    }
    println!(
        "froze {} ({} files) into {}; point [model.baseline] path at it",
        loaded.namespace,
        loaded.files.len(),
        out.display()
    );
    Ok(())
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
            let protocol = source
                .protocol
                .as_ref()
                .map(|id| {
                    crucible_smithy::Protocol::ALL
                        .iter()
                        .copied()
                        .find(|p| p.trait_id() == *id)
                        .ok_or_else(|| format!("unsupported protocol {id}"))
                })
                .transpose()?;
            let full = load_model(&source)?
                .lower_documentation(source.service.as_ref(), protocol)
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
        } => {
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
