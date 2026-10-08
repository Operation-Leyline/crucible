//! Compile a model once and generate named client and other model outputs. Enable the
//! `java` feature for this example; other clients have matching opt-in features.
//!
//! ```ignore
//! fn main() {
//!     let model = crucible_build::smithy("model").compile();
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
//! A separate SDK producer can use [`model_dependency`] without recompiling
//! Smithy. `.generate()` keeps artifacts under OUT_DIR for explicit export with
//! `crucible export-artifact`. `.generate_into()` also publishes a stable native
//! package path and watches its generated files so deleted exports regenerate.
//!
//! Defaults enable `smithy` and `rust`. Select `java` or other native client
//! features explicitly. Each native target has a matching fluent helper such as
//! `swift_client`, `python_client`, or `ts_client`. Select `openapi`, `cdk`,
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
//! fn main() { crucible_build::rust_service_from_smithy("smithy").generate(); }
//! ```
//!
//! A standalone Rust client uses the same compiler setup:
//!
//! ```ignore
//! fn main() { crucible_build::client_from_smithy("smithy").generate(); }
//! ```
//!
//! The default compiler and matching runtime are managed by the build helper.
//! Larger applications can share their contract through separate packages:
//!
//! A **model package** holds Smithy sources. Its manifest sets
//! `links = "<name>"`, and its `build.rs` compiles and publishes the AST:
//!
//! ```ignore
//! fn main() { crucible_build::publish_model_from_smithy("smithy"); }
//! ```
//!
//! For a precompiled AST, use `publish_model("model/model.json")` instead.
//!
//! A **service package** depends on the model package and generates the code
//! that plugs it into crucible:
//!
//! ```ignore
//! fn main() { crucible_build::rust_service("weather_model").generate(); }
//! ```
//!
//! and includes it with `crucible_server_runtime_rust::include_service!()`.
//!
//! A **client package** depends on the model package too, and generates a
//! client for it:
//!
//! ```ignore
//! fn main() { crucible_build::client("weather_model").generate(); }
//! ```
//!
//! and includes it with `crucible_client_runtime_rust::include_client!()`. Cargo pins the
//! model version like any other dependency.

use std::path::{Path, PathBuf};

#[cfg(feature = "rust")]
use crucible_codegen_rust::{ConformanceOptions, Options, Seed, Skip};
#[cfg(feature = "rust")]
use crucible_smithy::Model;
use crucible_smithy::ShapeId;

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
#[cfg(feature = "csharp")]
mod csharp_sdk;
#[cfg(feature = "dart")]
mod dart_sdk;
#[cfg(feature = "go")]
mod go_sdk;
#[cfg(feature = "java")]
mod java_sdk;
#[cfg(feature = "kotlin")]
mod kotlin_sdk;
mod model_builder;
mod model_package;
#[cfg(feature = "openapi")]
mod openapi_output;
mod output;
#[cfg(any(
    feature = "ts",
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
    feature = "cdk",
    feature = "terraform",
    feature = "cli"
))]
mod output_builder;
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
pub use crucible_codegen as clients;
pub use crucible_codegen as outputs;
pub use crucible_codegen::Audience;
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
pub use model_builder::ModelBuilder;
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
pub use smithy::client_from_smithy;
#[cfg(feature = "rust")]
pub use smithy::client_from_smithy_with;
#[cfg(feature = "smithy")]
pub use smithy::compile_smithy;
pub use smithy::compile_smithy_with;
#[cfg(feature = "smithy")]
pub use smithy::publish_model_from_smithy;
pub use smithy::publish_model_from_smithy_with;
#[cfg(all(feature = "smithy", feature = "rust"))]
pub use smithy::rust_service_from_smithy;
#[cfg(feature = "rust")]
pub use smithy::rust_service_from_smithy_with;

/// Compile a source-only Smithy package and select named generated outputs.
pub fn smithy(sources: impl AsRef<Path>) -> ModelBuilder {
    ModelBuilder::new(sources.as_ref(), false)
}

/// Use a compiled AST without invoking a Smithy compiler.
pub fn model_from_json(model: impl AsRef<Path>) -> ModelBuilder {
    ModelBuilder::new(model.as_ref(), true)
}

/// Generate outputs in a separate package using its model dependency's AST.
pub fn model_dependency(links: &str) -> ModelPackage {
    model_from_json(published(links)).compile()
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
pub fn publish_model(path: impl AsRef<Path>) {
    let path = absolute(path.as_ref());
    println!("cargo::rerun-if-changed={}", path.display());
    println!("cargo::metadata=model={}", path.display());
}

/// From a service package's `build.rs`: generates the server side from the
/// model published by the dependency whose manifest says `links = "<links>"`.
#[cfg(feature = "rust")]
pub fn rust_service(links: &str) -> RustCodegen {
    RustCodegen::from_model(published(links))
}

/// From a client package's `build.rs`: generates a client from the model
/// published by the dependency whose manifest says `links = "<links>"`.
#[cfg(feature = "rust")]
pub fn client(links: &str) -> RustCodegen {
    RustCodegen::client_from_model(published(links))
}

fn published(links: &str) -> PathBuf {
    let variable = format!("DEP_{}_MODEL", links.to_ascii_uppercase().replace('-', "_"));
    std::env::var_os(&variable).map(PathBuf::from).unwrap_or_else(|| {
        panic!(
            "crucible: {variable} is not set. Depend on the model package (a normal dependency, \
             whose manifest says `links = \"{links}\"`) and have its build.rs call \
             crucible_build::publish_model"
        )
    })
}

/// What to generate, and from which model.
#[derive(Debug, Clone)]
#[cfg(feature = "rust")]
pub struct RustCodegen {
    model: PathBuf,
    service: Option<ShapeId>,
    options: Options,
    side: Side,
    audience: Audience,
    policy: Option<PathBuf>,
    model_output: Option<PathBuf>,
    conformance: Option<ConformanceOptions>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(feature = "rust")]
enum Side {
    Server,
    Client,
}

#[cfg(feature = "rust")]
impl RustCodegen {
    /// The server side, from a model file directly: for a model inside the
    /// service package.
    pub fn from_model(model: impl AsRef<Path>) -> Self {
        Self {
            model: absolute(model.as_ref()),
            service: None,
            options: Options::default(),
            side: Side::Server,
            audience: Audience::External,
            policy: None,
            model_output: None,
            conformance: None,
        }
    }

    /// Select client visibility. Server generation always retains everything.
    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = audience;
        self
    }

    /// A client, from a model file directly.
    pub fn client_from_model(model: impl AsRef<Path>) -> Self {
        Self {
            side: Side::Client,
            ..Self::from_model(model)
        }
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

    /// Also generate the model's `smithy.test` vectors as server tests, into
    /// `OUT_DIR/crucible_conformance.rs`. Server side only. Request vectors
    /// run against a stub that captures the decoded input, response vectors
    /// have a stub return their `params`, and malformed-request vectors must
    /// be refused before any operation runs; the checks live in
    /// `crucible_server_runtime_rust::conformance`. Include the tests in the
    /// crate's test build with `crucible_server_runtime_rust::include_conformance!()`,
    /// with that crate's `test-support` feature enabled. A model without an
    /// applicable vector fails the build.
    pub fn conformance(mut self) -> Self {
        self.conformance.get_or_insert_with(Default::default);
        self
    }

    /// Keeps a vector the server is known to fail in the run as an ignored
    /// test carrying `reason`, instead of dropping it. `vector` is the id
    /// `crucible conformance` exports (`Id[name=value]` for a `testParameters` case,
    /// or the template id for all of its cases). A name matching no vector
    /// fails the build, so a skip cannot outlive its vector. Implies
    /// [`conformance`](Self::conformance).
    pub fn skip_vector(mut self, vector: &str, reason: &str) -> Self {
        self.conformance
            .get_or_insert_with(Default::default)
            .skip
            .push(Skip {
                vector: vector.to_owned(),
                reason: reason.to_owned(),
            });
        self
    }

    /// A request `operation` accepts, for the response vectors of an operation
    /// that has no request vector and cannot be called empty (an RPC operation,
    /// say: its request is a route and headers, not input). The headers it
    /// needs are the protocol's. Implies [`conformance`](Self::conformance).
    pub fn seed_request(
        mut self,
        operation: &str,
        method: &str,
        uri: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> Self {
        self.conformance
            .get_or_insert_with(Default::default)
            .seeds
            .push(Seed {
                operation: operation.to_owned(),
                method: method.to_owned(),
                uri: uri.to_owned(),
                headers: headers
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                    .collect(),
                body: body.map(str::to_owned),
            });
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
            Side::Client => "crucible_client_runtime_rust.rs",
        };
        std::fs::write(out.join(file), code).expect("crucible: cannot write the generated code");
        if self.conformance.is_some() {
            let tests = self
                .render_conformance()
                .unwrap_or_else(|message| panic!("crucible: {message}"));
            std::fs::write(out.join("crucible_conformance.rs"), tests)
                .expect("crucible: cannot write the generated conformance tests");
        }
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
        let json = std::fs::read_to_string(&self.model)
            .map_err(|error| format!("cannot read {}: {error}", self.model.display()))?;
        let model = Model::from_json(&json).map_err(|error| error.to_string())?;
        let capabilities = match self.side {
            Side::Server => crucible_codegen_rust::capabilities(),
            Side::Client => crucible_codegen_rust::client_capabilities(),
        };
        let service = model
            .lower_with_capabilities(
                self.service.as_ref(),
                &crucible_smithy::Capabilities::new(crucible_smithy::Feature::ALL.iter().copied()),
            )
            .map_err(|error| error.to_string())?;
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

    /// The conformance tests, or why there are none.
    pub fn render_conformance(&self) -> Result<String, String> {
        let options = self
            .conformance
            .as_ref()
            .ok_or("conformance tests were not requested: call `.conformance()`")?;
        if self.side != Side::Server {
            return Err("conformance tests test a server, not a client".to_owned());
        }
        crucible_codegen_rust::generate_conformance(&self.lower()?, options).map_err(
            |diagnostics| {
                format!(
                    "cannot generate conformance tests from {}:
{diagnostics}",
                    self.model.display()
                )
            },
        )
    }

    /// The generated code, or why there is none.
    pub fn render(&self) -> Result<String, String> {
        let service = self.lower()?;
        let generated = match self.side {
            Side::Server => crucible_codegen_rust::generate(&service, &self.options),
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
