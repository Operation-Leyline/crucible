//! Compile a model once and generate named client and other model outputs. Enable the
//! `java` feature for this example; other clients have matching opt-in features.
//!
//! ```ignore
//! fn main() {
//!     let model = crucible_build::model("model");
//!     model.publish();
//!     model.rust_client("rust")
//!         .package("weather-sdk")
//!         .generate_into("../target/rust-sdk");
//!     model.java_client("java")
//!         .package("example.sdk")
//!         .generate_into("../target/java-sdk");
//! }
//! ```
//!
//! Rust generation emits a complete Cargo package with a pinned runtime
//! dependency. Applications declare an ordinary path dependency on the exported
//! SDK and import its client and runtime API, without an inclusion macro.
//! Java generation emits Gradle and Maven metadata. Build native exports with
//! Cargo, Gradle or Maven; generation does not publish them to remote registries.
//! A separate SDK producer can use `model("<links>")` without recompiling
//! Smithy. `.generate()` keeps artifacts under OUT_DIR for explicit export with
//! `crucible export-artifact`. `.generate_into()` also publishes a stable native
//! package path and watches its generated files so deleted exports regenerate.
//!
//! Defaults enable `smithy` and `rust`. Select `java` or other native client
//! features explicitly. Each native target has a matching fluent helper such as
//! `swift_client`, `python_client`, or `ts_client`. Select `openapi`, `docs`, `cdk`,
//! `terraform`, or `cli` for named non-client outputs. With defaults disabled,
//! AST publication and model compilation remain usable without any Rust emitter
//! dependency. The `cli` target generates a Rust executable and uses the Rust emitter.
//!
//! ## Rust server integration and source-only clients
//!
//! The `rust` feature also retains direct generation into an existing crate.
//! Server helpers generate bindings inside the service crate. Source-only client
//! helpers are also available alongside native SDK package generation.
//!
//! Compile a service's Smithy sources and generate its Rust contract:
//!
//! ```ignore
//! fn main() { crucible_build::model("smithy").server().generate(); }
//! ```
//!
//! A standalone Rust client uses the same compiler setup:
//!
//! ```ignore
//! fn main() { crucible_build::model("smithy").client().generate(); }
//! ```
//!
//! The default compiler and matching runtime are managed by the build helper.
//! Larger applications can share their contract through separate packages:
//!
//! A **model package** holds Smithy sources. Its manifest sets
//! `links = "<name>"`, and its `build.rs` compiles and publishes the AST:
//!
//! ```ignore
//! fn main() { crucible_build::model("smithy").publish(); }
//! ```
//!
//! For a precompiled AST, use `model("model/model.json").publish()`.
//!
//! A **service package** depends on the model package and generates the code
//! that plugs it into crucible:
//!
//! ```ignore
//! fn main() { crucible_build::model("weather_model").server().generate(); }
//! ```
//!
//! and includes it with `crucible_server_runtime_rust::include_service!()`.
//!
//! A **client package** depends on the model package too, and generates a
//! client for it:
//!
//! ```ignore
//! fn main() { crucible_build::model("weather_model").client().generate(); }
//! ```
//!
//! and includes it with `crucible_client_runtime_rust::include_client!()`. Cargo pins the
//! model version like any other dependency.

use std::path::{Path, PathBuf};

#[cfg(feature = "rust")]
use crucible_codegen_rust::Options;
#[cfg(feature = "rust")]
use crucible_smithy::Model as SmithyModel;
use crucible_smithy::ShapeId;

#[cfg(feature = "rust")]
mod client_output;
#[cfg(feature = "rust")]
pub use client_output::ClientOutput;

mod artifact;
mod artifact_index;
#[cfg(feature = "c")]
mod c_sdk;
#[cfg(feature = "cdk")]
mod cdk_output;
#[cfg(feature = "cli")]
mod cli_output;
#[cfg(feature = "cpp")]
mod cpp_sdk;
mod cru;
#[cfg(feature = "csharp")]
mod csharp_sdk;
#[cfg(feature = "dart")]
mod dart_sdk;
mod declared_sdk;
#[cfg(feature = "docs")]
mod docs_output;
#[cfg(feature = "go")]
mod go_sdk;
#[cfg(feature = "java")]
mod java_sdk;
#[cfg(feature = "kotlin")]
mod kotlin_sdk;
mod model;
mod model_builder;
mod model_package;
#[cfg(feature = "openapi")]
mod openapi_output;
mod output;
#[cfg(any(
    feature = "python",
    feature = "go",
    feature = "csharp",
    feature = "kotlin",
    feature = "swift",
    feature = "dart",
    feature = "php",
    feature = "ruby",
    feature = "cpp",
    feature = "c",
    feature = "r",
    feature = "openapi",
    feature = "docs",
    feature = "cdk",
    feature = "terraform",
    feature = "cli"
))]
mod output_builder;
#[cfg(any(feature = "rust", feature = "ts"))]
mod package_metadata;
#[cfg(feature = "php")]
mod php_sdk;
#[cfg(any(feature = "cdk", feature = "terraform"))]
mod provisioning_bindings;
#[cfg(feature = "python")]
mod python_sdk;
#[cfg(feature = "r")]
mod r_sdk;
#[cfg(feature = "ruby")]
mod ruby_sdk;
#[cfg(feature = "rust")]
mod rust_sdk;
#[cfg(feature = "swift")]
mod swift_sdk;
#[cfg(feature = "terraform")]
mod terraform_output;
#[cfg(feature = "ts")]
mod ts_sdk;
pub use artifact::Artifact;
pub use artifact_index::export_artifact;
#[cfg(feature = "c")]
pub use c_sdk::CSdk;
#[cfg(feature = "cdk")]
pub use cdk_output::CdkOutput;
#[cfg(feature = "cli")]
pub use cli_output::CliOutput;
#[cfg(feature = "cpp")]
pub use cpp_sdk::CppSdk;
#[cfg(feature = "c")]
pub use crucible_codegen::COptions;
#[cfg(feature = "cli")]
pub use crucible_codegen::CliOptions;
#[cfg(feature = "cpp")]
pub use crucible_codegen::CppOptions;
#[cfg(feature = "csharp")]
pub use crucible_codegen::CsharpOptions;
#[cfg(feature = "dart")]
pub use crucible_codegen::DartOptions;
#[cfg(feature = "go")]
pub use crucible_codegen::GoOptions;
#[cfg(feature = "java")]
pub use crucible_codegen::JavaOptions;
#[cfg(feature = "kotlin")]
pub use crucible_codegen::KotlinOptions;
#[cfg(feature = "openapi")]
pub use crucible_codegen::OpenApiOptions;
#[cfg(feature = "php")]
pub use crucible_codegen::PhpOptions;
#[cfg(any(feature = "cdk", feature = "terraform"))]
pub use crucible_codegen::ProvisioningBindings;
#[cfg(feature = "python")]
pub use crucible_codegen::PythonOptions;
#[cfg(feature = "r")]
pub use crucible_codegen::ROptions;
#[cfg(feature = "ruby")]
pub use crucible_codegen::RubyOptions;
#[cfg(feature = "rust")]
pub use crucible_codegen::RustServerOptions;
#[cfg(feature = "swift")]
pub use crucible_codegen::SwiftOptions;
#[cfg(feature = "terraform")]
pub use crucible_codegen::TerraformOptions;
#[cfg(feature = "ts")]
pub use crucible_codegen::TsMiddleware;
#[cfg(feature = "ts")]
pub use crucible_codegen::TsOptions;
#[cfg(feature = "cdk")]
pub use crucible_codegen::cdk;
#[cfg(any(feature = "cdk", feature = "terraform"))]
pub use crucible_codegen::infra;
pub use crucible_codegen::{Audience, Client, Generator, PackageMetadata, Registry, Server};
#[cfg(feature = "cdk")]
pub use crucible_codegen::{CdkOptions, CdkTargets};
#[cfg(feature = "rust")]
pub use crucible_codegen::{RuntimeDependency, RustOptions};

#[deprecated(
    since = "0.5.0",
    note = "use crucible_build::Generator and native options directly"
)]
/// Deprecated generator/options namespace. Audience has one public import path.
///
/// ```compile_fail
/// use crucible_build::outputs::Audience;
/// ```
pub mod outputs {
    #[cfg(feature = "c")]
    pub use super::COptions;
    #[cfg(feature = "cli")]
    pub use super::CliOptions;
    #[cfg(feature = "cpp")]
    pub use super::CppOptions;
    #[cfg(feature = "csharp")]
    pub use super::CsharpOptions;
    #[cfg(feature = "dart")]
    pub use super::DartOptions;
    #[cfg(feature = "go")]
    pub use super::GoOptions;
    #[cfg(feature = "java")]
    pub use super::JavaOptions;
    #[cfg(feature = "kotlin")]
    pub use super::KotlinOptions;
    #[cfg(feature = "openapi")]
    pub use super::OpenApiOptions;
    #[cfg(feature = "php")]
    pub use super::PhpOptions;
    #[cfg(any(feature = "cdk", feature = "terraform"))]
    pub use super::ProvisioningBindings;
    #[cfg(feature = "python")]
    pub use super::PythonOptions;
    #[cfg(feature = "r")]
    pub use super::ROptions;
    #[cfg(feature = "ruby")]
    pub use super::RubyOptions;
    #[cfg(feature = "rust")]
    pub use super::RustServerOptions;
    #[cfg(feature = "swift")]
    pub use super::SwiftOptions;
    #[cfg(feature = "terraform")]
    pub use super::TerraformOptions;
    #[cfg(feature = "ts")]
    pub use super::TsMiddleware;
    #[cfg(feature = "ts")]
    pub use super::TsOptions;
    #[cfg(feature = "cdk")]
    pub use super::cdk;
    #[cfg(any(feature = "cdk", feature = "terraform"))]
    pub use super::infra;
    #[cfg(feature = "cdk")]
    pub use super::{CdkOptions, CdkTargets};
    pub use super::{Client, Generator, Server};
    #[cfg(feature = "rust")]
    pub use super::{RuntimeDependency, RustOptions};
    pub use crucible_codegen::{File, write};
}

#[cfg(feature = "csharp")]
pub use csharp_sdk::CsharpSdk;
#[cfg(feature = "dart")]
pub use dart_sdk::DartSdk;
#[cfg(feature = "go")]
pub use go_sdk::GoSdk;
#[cfg(feature = "java")]
pub use java_sdk::JavaSdk;
#[cfg(feature = "kotlin")]
pub use kotlin_sdk::KotlinSdk;
pub use model::Model;
#[deprecated(since = "0.5.0", note = "use crucible_build::model(..)")]
pub type ModelBuilder = Model;
#[cfg(feature = "docs")]
pub use docs_output::DocsOutput;
pub use model_package::ModelPackage;
#[cfg(feature = "openapi")]
pub use openapi_output::OpenApiOutput;
pub use output::Output;
#[cfg(feature = "php")]
pub use php_sdk::PhpSdk;
#[cfg(feature = "python")]
pub use python_sdk::PythonSdk;
#[cfg(feature = "r")]
pub use r_sdk::RSdk;
#[cfg(feature = "ruby")]
pub use ruby_sdk::RubySdk;
#[cfg(feature = "rust")]
pub use rust_sdk::RustSdk;
#[cfg(feature = "terraform")]
pub use terraform_output::TerraformOutput;

#[cfg(feature = "swift")]
pub use swift_sdk::SwiftSdk;
#[cfg(feature = "ts")]
pub use ts_sdk::TsSdk;

mod smithy;
#[cfg(all(feature = "smithy", feature = "rust"))]
#[allow(deprecated)]
pub use smithy::client_from_smithy;
#[cfg(feature = "rust")]
#[allow(deprecated)]
pub use smithy::client_from_smithy_with;
#[cfg(feature = "smithy")]
#[allow(deprecated)]
pub use smithy::compile_smithy;
#[allow(deprecated)]
pub use smithy::compile_smithy_with;
#[cfg(feature = "smithy")]
#[allow(deprecated)]
pub use smithy::publish_model_from_smithy;
#[allow(deprecated)]
pub use smithy::publish_model_from_smithy_with;
#[cfg(all(feature = "smithy", feature = "rust"))]
#[allow(deprecated)]
pub use smithy::rust_service_from_smithy;
#[cfg(feature = "rust")]
#[allow(deprecated)]
pub use smithy::rust_service_from_smithy_with;

/// Compile a source-only Smithy package and select named generated outputs.
#[deprecated(since = "0.5.0", note = "use crucible_build::model(..)")]
pub fn smithy(sources: impl AsRef<Path>) -> Model {
    Model::from_sources(sources)
}

/// Use a compiled AST without invoking a Smithy compiler.
#[deprecated(since = "0.5.0", note = "use crucible_build::model(..)")]
pub fn model_from_json(source: impl AsRef<Path>) -> Model {
    Model::from_json(source)
}

/// Generate outputs in a separate package using its model dependency's AST.
#[deprecated(since = "0.5.0", note = "use crucible_build::model(..)")]
pub fn model_dependency(links: &str) -> ModelPackage {
    Model::from_dependency(links).compile()
}

/// Resolve a named artifact published by a direct model dependency.
pub fn artifact(links: &str, name: &str) -> Artifact {
    artifact::validate_name(name).unwrap_or_else(|message| panic!("crucible: {message}"));
    let variable = format!(
        "DEP_{}_ARTIFACT_{}",
        links.to_ascii_uppercase().replace('-', "_"),
        name.to_ascii_uppercase().replace('-', "_")
    );
    let path = std::env::var_os(&variable).map(PathBuf::from).unwrap_or_else(|| panic!(
        "crucible: {variable} is not set. Depend directly on the model package with links = {links:?} and generate its named output {name:?}"
    ));
    let index_path = path
        .parent()
        .and_then(Path::parent)
        .expect("artifact parent")
        .join("crucible-artifacts.json");
    let artifact = artifact_index::load(&index_path, name)
        .unwrap_or_else(|message| panic!("crucible: {message}"));
    assert_eq!(
        artifact.path, path,
        "crucible: artifact registry does not match Cargo metadata"
    );
    artifact
}

/// From a model package's `build.rs`: makes `path` (a Smithy JSON AST built
/// with the `flattenAndRemoveMixins` transform) the model its dependents
/// generate from. The package's manifest needs `links`.
#[deprecated(since = "0.5.0", note = "use crucible_build::model(..).publish()")]
pub fn publish_model(path: impl AsRef<Path>) {
    Model::from_json(path).publish();
}

fn publish_path(path: &Path) {
    println!("cargo::rerun-if-changed={}", path.display());
    println!("cargo::metadata=model={}", path.display());
}

/// From a service package's `build.rs`: generates the server side from the
/// model published by the dependency whose manifest says `links = "<links>"`.
#[cfg(feature = "rust")]
#[deprecated(since = "0.5.0", note = "use crucible_build::model(..).server()")]
pub fn rust_service(links: &str) -> ServerOutput {
    Model::from_dependency(links).server()
}

/// From a client package's `build.rs`: generates a client from the model
/// published by the dependency whose manifest says `links = "<links>"`.
#[cfg(feature = "rust")]
#[deprecated(since = "0.5.0", note = "use crucible_build::model(..).client()")]
pub fn client(links: &str) -> ServerOutput {
    Model::from_dependency(links).client().into_legacy()
}

/// A model dependency is missing Cargo's published AST metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingModelDependency {
    pub links: String,
    pub variable: String,
}

impl std::fmt::Display for MissingModelDependency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "crucible: model dependency {:?} is missing {}. Add the model package to [dependencies] in Cargo.toml; its manifest must declare links = {:?} and its build.rs must call crucible_build::model(..).publish()",
            self.links, self.variable, self.links
        )
    }
}
impl std::error::Error for MissingModelDependency {}

fn try_published(links: &str) -> Result<PathBuf, MissingModelDependency> {
    let variable = format!("DEP_{}_MODEL", links.to_ascii_uppercase().replace('-', "_"));
    std::env::var_os(&variable)
        .map(PathBuf::from)
        .ok_or_else(|| MissingModelDependency {
            links: links.to_owned(),
            variable,
        })
}

/// Select Smithy sources, a JSON AST, or a direct model dependency's `links` name.
pub fn model(source: impl AsRef<Path>) -> Model {
    Model::new(source.as_ref())
}

/// Deprecated name for the Rust binding builder.
#[cfg(feature = "rust")]
#[deprecated(
    since = "0.5.0",
    note = "use crucible_build::model(..).server() or .client()"
)]
pub type RustCodegen = ServerOutput;

/// Deprecated dependency-based server entry point.
#[cfg(feature = "rust")]
#[deprecated(since = "0.5.0", note = "use crucible_build::model(..).server()")]
pub fn service(links: &str) -> ServerOutput {
    Model::from_dependency(links).server()
}

/// What to generate, and from which model.
#[derive(Debug, Clone)]
#[cfg(feature = "rust")]
pub struct ServerOutput {
    model: PathBuf,
    pub(crate) service: Option<ShapeId>,
    options: Options,
    side: Side,
    audience: Audience,
    explorer_audience: Audience,
    pub(crate) policy: Option<PathBuf>,
    model_output: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(feature = "rust")]
enum Side {
    Server,
    Client,
}

#[cfg(feature = "rust")]
impl ServerOutput {
    /// The server side, from a model file directly: for a model inside the
    /// service package.
    fn new(model: impl AsRef<Path>) -> Self {
        Self {
            model: absolute(model.as_ref()),
            service: None,
            options: Options::default(),
            side: Side::Server,
            audience: Audience::External,
            explorer_audience: Audience::Internal,
            policy: None,
            model_output: None,
        }
    }

    /// Deprecated direct server constructor.
    #[deprecated(since = "0.5.0", note = "use crucible_build::model(..).server()")]
    pub fn from_model(source: impl AsRef<Path>) -> Self {
        Model::from_json(source).server()
    }

    /// Select client or Explorer visibility. Explorer defaults to Internal;
    /// normal server operations always retain everything.
    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = audience;
        self.explorer_audience = audience;
        if self.options.explorer.is_some() {
            self.options.explorer = Some(audience);
        }
        self
    }

    /// A client, from a model file directly.
    #[deprecated(since = "0.5.0", note = "use crucible_build::model(..).client()")]
    pub fn client_from_model(source: impl AsRef<Path>) -> Self {
        Model::from_json(source).client().into_legacy()
    }

    /// The service to generate, when the model defines several.
    pub fn named(mut self, id: &str) -> Self {
        self.service = Some(parse(id));
        self
    }

    /// Select a required policy file relative to this package's Cargo.toml.
    /// A workspace can share one file by selecting it explicitly in each build script.
    pub fn policy(mut self, path: impl AsRef<Path>) -> Self {
        self.policy = Some(absolute(path.as_ref()));
        self
    }

    /// Export the assembled AST for compatibility checks. Prefer a path in OUT_DIR
    /// for normal builds; CI can select a stable path through CRUCIBLE_MODEL_OUT.
    pub fn model_output(mut self, path: impl AsRef<Path>) -> Self {
        self.model_output = Some(absolute(path.as_ref()));
        self
    }

    /// The error a rejected request becomes. Server side only.
    pub fn validation_error(mut self, id: &str) -> Self {
        self.options.validation_error = Some(parse(id));
        self
    }

    /// Generate static External-audience MCP descriptors; enable the protocol at runtime.
    pub fn mcp(mut self) -> Self {
        self.options.mcp = true;
        self
    }
    /// Embed a static `DESCRIPTION` for the local-development Explorer.
    /// Internal audience is the default; `.audience(..)` changes the description
    /// without removing any normal server operation. Enable the runtime's
    /// `explorer` Cargo feature in the service.
    pub fn explorer(mut self) -> Self {
        self.options.explorer = Some(self.explorer_audience);
        self
    }

    /// Every structure an input reaches refuses unknown members, not only
    /// those marked `@crucible.protocols#strict`. Server side only.
    pub fn strict_inputs(mut self) -> Self {
        self.options.strict_inputs = true;
        self
    }

    /// The error internal failures are answered with. Server side only.
    pub fn internal_error(mut self, id: &str) -> Self {
        self.options.internal_error = Some(parse(id));
        self
    }

    /// The modelled HTTP 404 error for an unmatched route. Server side only.
    /// By default, uses `crucible.errors#UnknownOperationException` when declared.
    pub fn unknown_operation_error(mut self, id: &str) -> Self {
        self.options.unknown_operation_error = Some(parse(id));
        self
    }

    /// An advanced service supplies `Crucible::builder(...).on_rejection(...)`
    /// itself. Disable the early rejection-error check for that service.
    pub fn custom_rejections(mut self) -> Self {
        self.options.require_rejection = false;
        self
    }

    /// Writes the code into `OUT_DIR`. A model crucible cannot generate from
    /// fails the build with every reason listed.
    pub fn generate(self) {
        println!("cargo::rerun-if-changed={}", self.model.display());
        if let Some(policy) = &self.policy {
            println!("cargo::rerun-if-changed={}", policy.display());
        }
        println!("cargo::rerun-if-env-changed=CRUCIBLE_MODEL_OUT");
        let code = self
            .render()
            .unwrap_or_else(|message| panic!("crucible: {message}"));
        let out =
            PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR: run from a build script"));
        let file = match self.side {
            Side::Server => "crucible_service.rs",
            Side::Client => "crucible_client.rs",
        };
        std::fs::write(out.join(file), code).expect("crucible: cannot write the generated code");
        if self.side == Side::Client {
            println!(
                "cargo::rustc-env=CRUCIBLE_CLIENT_SOURCE={}",
                out.join(file).display()
            );
        }
        let model_output = self.model_output.clone().or_else(|| {
            std::env::var_os("CRUCIBLE_MODEL_OUT")
                .map(PathBuf::from)
                .map(|path| absolute(&path))
        });
        if let Some(path) = model_output {
            assert!(
                !cru::is_package(&self.model),
                "crucible: exporting the model of a .cru package ({}) for compatibility checks is not supported yet",
                self.model.display()
            );
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .expect("crucible: cannot create model export directory");
            }
            std::fs::copy(&self.model, &path).unwrap_or_else(|error| {
                panic!(
                    "crucible: cannot export model to {}: {error}",
                    path.display()
                )
            });
            // Copy preserves the source timestamp on Windows but not Linux.
            // A newer watched export would invalidate Cargo's own build result.
            let modified = std::fs::metadata(&self.model)
                .and_then(|metadata| metadata.modified())
                .expect("crucible: cannot read model timestamp");
            std::fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|file| file.set_modified(modified))
                .expect("crucible: cannot preserve model export timestamp");
            println!("cargo::rerun-if-changed={}", path.display());
        }
    }

    fn lower(&self) -> Result<crucible_smithy::ir::Service, String> {
        let capabilities = match self.side {
            Side::Server => crucible_codegen_rust::capabilities(),
            Side::Client => crucible_codegen_rust::client_capabilities(),
        };
        let service = if cru::is_package(&self.model) {
            cru::lower(&self.model, self.service.as_ref())?
        } else {
            let json = std::fs::read_to_string(&self.model)
                .map_err(|error| format!("cannot read {}: {error}", self.model.display()))?;
            let model = SmithyModel::from_json(&json).map_err(|error| error.to_string())?;
            let capabilities_all =
                crucible_smithy::Capabilities::new(crucible_smithy::Feature::ALL.iter().copied());
            model
                .lower_with_capabilities(self.service.as_ref(), &capabilities_all)
                .map_err(|error| error.to_string())?
        };
        if let Some(policy) = &self.policy {
            check_policy(&service, policy)?;
        }
        let service = if self.side == Side::Client {
            crucible_smithy::project(&service, self.audience).map_err(|e| e.to_string())?
        } else {
            service
        };
        service
            .require_capabilities(&capabilities)
            .map_err(|e| e.to_string())?;
        Ok(service)
    }

    /// The generated code, or why there is none.
    pub fn render(&self) -> Result<String, String> {
        let service = self.lower()?;
        if self.options.mcp && self.side == Side::Server {
            let external = crucible_smithy::project(&service, crucible_smithy::Audience::External)
                .map_err(|d| d.to_string())?;
            for note in crucible_codegen_rust::mcp_exclusions(&external) {
                println!("cargo::warning={note}");
            }
        }
        let generated = match self.side {
            Side::Server => {
                let explorer_service = if self.options.explorer.is_some() {
                    if cru::is_package(&self.model) {
                        return Err(cru::unsupported("An Explorer description", &self.model));
                    }
                    let json = std::fs::read_to_string(&self.model).map_err(|e| e.to_string())?;
                    let model = SmithyModel::from_json(&json).map_err(|e| e.to_string())?;
                    Some(
                        model
                            .lower_documentation(self.service.as_ref())
                            .map_err(|e| e.to_string())?,
                    )
                } else {
                    None
                };
                crucible_codegen_rust::generate_with_explorer_model(
                    &service,
                    &self.options,
                    explorer_service.as_ref().unwrap_or(&service),
                )
            }
            Side::Client => crucible_codegen_rust::generate_client(&service),
        };
        generated.map_err(|diagnostics| {
            format!(
                "cannot generate from {}:\n{diagnostics}",
                self.model.display()
            )
        })
    }
}

fn parse(id: &str) -> ShapeId {
    id.parse()
        .unwrap_or_else(|error| panic!("crucible: {error}"))
}

/// Relative paths are the package's, as Cargo runs build scripts from it.
fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let root =
        std::env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);
    root.join(path)
}

fn check_policy(service: &crucible_smithy::ir::Service, policy: &Path) -> Result<(), String> {
    let config = crucible_policy::Config::load(policy)?;
    let report = crucible_policy::check(service, &config);
    for finding in &report.findings {
        if let Some(reason) = &finding.exception {
            println!(
                "cargo::warning=excepted[{}] {}: {}",
                finding.rule,
                finding.shape,
                reason.replace(['\n', '\r'], " ")
            );
        }
        if finding.severity == crucible_policy::Severity::Warning && finding.exception.is_none() {
            println!(
                "cargo::warning=[{}] {}: {}",
                finding.rule,
                finding.shape,
                finding.message.replace(['\n', '\r'], " ")
            );
        }
    }
    if report.has_errors() {
        return Err(report.to_string());
    }
    Ok(())
}
